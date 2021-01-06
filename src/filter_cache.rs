use super::*;
use std::collections::HashMap;

const VERSION: u64 = 1;

lazy_static! {
    static ref DB: std::sync::Mutex<Option<sled::Db>> =
        std::sync::Mutex::new(None);
}

pub fn load(path: &std::path::Path) -> JoshResult<()> {
    *DB.lock()? = Some(
        sled::Config::default()
            .path(path.join(format!("josh/{}/sled/", VERSION)))
            .flush_every_ms(Some(200))
            .open()?,
    );
    Ok(())
}

pub fn print_stats() {
    let d = DB.lock().unwrap();
    let db = d.as_ref().unwrap();
    db.flush().unwrap();
    log::debug!("Trees:");
    let mut v = vec![];
    for name in db.tree_names() {
        let name = String::from_utf8(name.to_vec()).unwrap();
        let t = db.open_tree(&name).unwrap();
        if t.len() != 0 {
            let name = if name.contains("SUBTRACT") {
                name.clone()
            } else {
                super::filters::pretty(super::filters::parse(&name).unwrap(), 4)
            };
            v.push((t.len(), name));
        }
    }

    v.sort();

    for (len, name) in v.iter() {
        println!("[{}] {}", len, name);
    }
}

pub struct Transaction<'a> {
    maps: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    trees: HashMap<git2::Oid, sled::Tree>,
    pub misses: usize,
    pub walks: usize,

    repo: &'a git2::Repository,
}

impl<'a> Transaction<'a> {
    pub fn new(repo: &'a git2::Repository) -> Transaction<'a> {
        log::debug!("new transaction");
        Transaction {
            maps: HashMap::new(),
            trees: HashMap::new(),
            repo: repo,
            misses: 0,
            walks: 0,
        }
    }

    pub fn insert(
        &mut self,
        filter: filters::Filter,
        from: git2::Oid,
        to: git2::Oid,
        store: bool,
    ) {
        self.maps
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);

        if store {
            let t = self.trees.entry(filter.id()).or_insert_with(|| {
                DB.lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .open_tree(filters::spec(filter))
                    .unwrap()
            });

            t.insert(from.as_bytes(), to.as_bytes()).unwrap();
        }
    }

    pub fn get(
        &mut self,
        filter: filters::Filter,
        from: git2::Oid,
    ) -> Option<git2::Oid> {
        if filter.is_nop() {
            return Some(from);
        }
        if let Some(m) = self.maps.get(&filter.id()) {
            if let Some(oid) = m.get(&from).cloned() {
                return Some(oid);
            }
        }
        let t = self.trees.entry(filter.id()).or_insert_with(|| {
            DB.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .open_tree(filters::spec(filter))
                .unwrap()
        });
        if let Some(oid) = t.get(from.as_bytes()).unwrap() {
            let oid = git2::Oid::from_bytes(&oid).unwrap();
            if oid == git2::Oid::zero() {
                return Some(oid);
            }
            if self.repo.odb().unwrap().exists(oid) {
                // Only report an object as cached if it exists in the object database.
                // This forces a rebuild in case the object was garbage collected.
                return Some(oid);
            }
        }

        self.misses += 1;

        return None;
    }
}
