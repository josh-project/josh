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
                filter::pretty(filter::parse(&name).unwrap(), 4)
            };
            v.push((t.len(), name));
        }
    }

    v.sort();

    for (len, name) in v.iter() {
        println!("[{}] {}", len, name);
    }
}

struct Transaction2 {
    commit_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    apply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    unapply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    sled_trees: HashMap<git2::Oid, sled::Tree>,
    misses: usize,
    walks: usize,
}

pub struct Transaction {
    t2: std::cell::RefCell<Transaction2>,
    repo: git2::Repository,
}

impl Transaction {
    pub fn new(repo: git2::Repository) -> Transaction {
        log::debug!("new transaction");
        Transaction {
            t2: std::cell::RefCell::new(Transaction2 {
                commit_map: HashMap::new(),
                apply_map: HashMap::new(),
                unapply_map: HashMap::new(),
                sled_trees: HashMap::new(),
                misses: 0,
                walks: 0,
            }),
            repo: repo,
        }
    }

    pub fn open(path: &std::path::Path) -> JoshResult<Transaction> {
        log::debug!("open transaction");
        Ok(Transaction {
            t2: std::cell::RefCell::new(Transaction2 {
                commit_map: HashMap::new(),
                apply_map: HashMap::new(),
                unapply_map: HashMap::new(),
                sled_trees: HashMap::new(),
                misses: 0,
                walks: 0,
            }),
            repo: git2::Repository::open_ext(
                path,
                git2::RepositoryOpenFlags::NO_SEARCH,
                &[] as &[&std::ffi::OsStr],
            )?,
        })
    }

    pub fn clone(&self) -> JoshResult<Transaction> {
        Transaction::open(self.repo.path())
    }

    pub fn repo(&self) -> &git2::Repository {
        &self.repo
    }

    pub fn misses(&self) -> usize {
        self.t2.borrow().misses
    }

    pub fn new_walk(&self) -> usize {
        let prev = self.t2.borrow().walks;
        self.t2.borrow_mut().walks += 1;
        return prev;
    }

    pub fn end_walk(&self) {
        self.t2.borrow_mut().walks -= 1;
    }

    pub fn insert_apply(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.apply_map
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);
    }

    pub fn get_apply(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
    ) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.apply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        return None;
    }

    pub fn insert_unapply(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.unapply_map
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);
    }

    pub fn get_unapply(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
    ) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.unapply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        return None;
    }

    pub fn insert(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
        store: bool,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.commit_map
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);

        if store {
            let t = t2.sled_trees.entry(filter.id()).or_insert_with(|| {
                DB.lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .open_tree(filter::spec(filter))
                    .unwrap()
            });

            t.insert(from.as_bytes(), to.as_bytes()).unwrap();
        }
    }
    pub fn len(&self, filter: filter::Filter) -> usize {
        let mut t2 = self.t2.borrow_mut();
        let t = t2.sled_trees.entry(filter.id()).or_insert_with(|| {
            DB.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .open_tree(filter::spec(filter))
                .unwrap()
        });

        return t.len();
    }
    pub fn get(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
    ) -> Option<git2::Oid> {
        if filter.is_nop() {
            return Some(from);
        }
        let mut t2 = self.t2.borrow_mut();
        if let Some(m) = t2.commit_map.get(&filter.id()) {
            if let Some(oid) = m.get(&from).cloned() {
                return Some(oid);
            }
        }
        let t = t2.sled_trees.entry(filter.id()).or_insert_with(|| {
            DB.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .open_tree(filter::spec(filter))
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

        t2.misses += 1;

        return None;
    }
}
