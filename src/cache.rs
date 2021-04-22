use super::*;
use std::collections::HashMap;

const VERSION: u64 = 5;

lazy_static! {
    static ref DB: std::sync::Mutex<Option<sled::Db>> = std::sync::Mutex::new(None);
    static ref REF_CACHE: std::sync::Mutex<HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>> =
        std::sync::Mutex::new(HashMap::new());
    static ref POPULATE_MAP: std::sync::Mutex<HashMap<(git2::Oid, git2::Oid), git2::Oid>> =
        std::sync::Mutex::new(HashMap::new());
    static ref GLOB_MAP: std::sync::Mutex<HashMap<(git2::Oid, git2::Oid), git2::Oid>> =
        std::sync::Mutex::new(HashMap::new());
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
            } else if name.starts_with("_") {
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

#[allow(unused)]
struct Transaction2 {
    commit_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    apply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    unapply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    sled_trees: HashMap<git2::Oid, sled::Tree>,
    path_tree: sled::Tree,
    invert_tree: sled::Tree,
    missing: Vec<(filter::Filter, git2::Oid)>,
    misses: usize,
    walks: usize,
}

pub struct Transaction {
    t2: std::cell::RefCell<Transaction2>,
    repo: git2::Repository,
    ref_prefix: String,
}

impl Transaction {
    pub fn open(path: &std::path::Path, ref_prefix: Option<&str>) -> JoshResult<Transaction> {
        Ok(Transaction::new(
            git2::Repository::open_ext(
                path,
                git2::RepositoryOpenFlags::NO_SEARCH,
                &[] as &[&std::ffi::OsStr],
            )?,
            ref_prefix,
        ))
    }

    pub fn status(&self, _msg: &str) {
        /* let mut t2 = self.t2.borrow_mut(); */
        /* write!(t2.out, "{}", msg).ok(); */
        /* t2.out.flush().ok(); */
    }

    pub fn new(repo: git2::Repository, ref_prefix: Option<&str>) -> Transaction {
        log::debug!("new transaction");
        let path_tree = DB
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .open_tree("_paths")
            .unwrap();
        let invert_tree = DB
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .open_tree("_invert")
            .unwrap();
        Transaction {
            t2: std::cell::RefCell::new(Transaction2 {
                commit_map: HashMap::new(),
                apply_map: HashMap::new(),
                unapply_map: HashMap::new(),
                sled_trees: HashMap::new(),
                path_tree: path_tree,
                invert_tree: invert_tree,
                missing: vec![],
                misses: 0,
                walks: 0,
            }),
            repo: repo,
            ref_prefix: ref_prefix.unwrap_or("").to_string(),
        }
    }

    pub fn clone(&self) -> JoshResult<Transaction> {
        Transaction::open(self.repo.path(), Some(&self.ref_prefix))
    }

    pub fn repo(&self) -> &git2::Repository {
        &self.repo
    }

    pub fn refname(&self, r: &str) -> String {
        format!("{}{}", self.ref_prefix, r)
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

    pub fn insert_apply(&self, filter: filter::Filter, from: git2::Oid, to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.apply_map
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);
    }

    pub fn get_apply(&self, filter: filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.apply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        return None;
    }

    pub fn insert_unapply(&self, filter: filter::Filter, from: git2::Oid, to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.unapply_map
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);
    }

    pub fn insert_paths(&self, tree: (git2::Oid, String), result: git2::Oid) {
        let t2 = self.t2.borrow();
        let s = format!("{:?}", tree);
        let x = git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes()).expect("hash_object");
        t2.path_tree
            .insert(x.as_bytes(), result.as_bytes())
            .unwrap();
    }

    pub fn get_paths(&self, tree: (git2::Oid, String)) -> Option<git2::Oid> {
        let t2 = self.t2.borrow();
        let s = format!("{:?}", tree);
        let x = git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes()).expect("hash_object");

        if let Some(oid) = t2.path_tree.get(x.as_bytes()).unwrap() {
            return Some(git2::Oid::from_bytes(&oid).unwrap());
        }
        return None;
    }

    pub fn insert_invert(&self, tree: (git2::Oid, String), result: git2::Oid) {
        let t2 = self.t2.borrow();
        let s = format!("{:?}", tree);
        let x = git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes()).expect("hash_object");
        t2.invert_tree
            .insert(x.as_bytes(), result.as_bytes())
            .unwrap();
    }

    pub fn get_invert(&self, tree: (git2::Oid, String)) -> Option<git2::Oid> {
        let t2 = self.t2.borrow();
        let s = format!("{:?}", tree);
        let x = git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes()).expect("hash_object");

        if let Some(oid) = t2.invert_tree.get(x.as_bytes()).unwrap() {
            return Some(git2::Oid::from_bytes(&oid).unwrap());
        }
        return None;
    }

    pub fn insert_populate(&self, tree: (git2::Oid, git2::Oid), result: git2::Oid) {
        POPULATE_MAP.lock().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_populate(&self, tree: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        return POPULATE_MAP.lock().unwrap().get(&tree).cloned();
    }

    pub fn insert_glob(&self, tree: (git2::Oid, git2::Oid), result: git2::Oid) {
        GLOB_MAP.lock().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_glob(&self, tree: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        return GLOB_MAP.lock().unwrap().get(&tree).cloned();
    }

    pub fn insert_ref(&self, filter: filter::Filter, from: git2::Oid, to: git2::Oid) {
        REF_CACHE
            .lock()
            .unwrap()
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);
    }

    pub fn get_ref(&self, filter: filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        if let Some(m) = REF_CACHE.lock().unwrap().get(&filter.id()) {
            if let Some(oid) = m.get(&from) {
                if self.repo.odb().unwrap().exists(*oid) {
                    return Some(*oid);
                }
            }
        }
        return None;
    }

    pub fn get_unapply(&self, filter: filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.unapply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        return None;
    }

    pub fn insert(&self, filter: filter::Filter, from: git2::Oid, to: git2::Oid, store: bool) {
        let mut t2 = self.t2.borrow_mut();
        t2.commit_map
            .entry(filter.id())
            .or_insert_with(|| HashMap::new())
            .insert(from, to);

        // In addition to commits that are explicitly requested to be stored, also store
        // random extra commits (probability 1/256) to avoid long searches for filters that reduce
        // the history length by a very large factor.
        if store || from.as_bytes()[0] == 0 {
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

    pub fn get_missing(&self) -> Vec<(filter::Filter, git2::Oid)> {
        let mut missing = self.t2.borrow().missing.clone();
        missing.sort();
        missing.dedup();
        missing.retain(|(f, i)| !self.known(*f, *i));
        self.t2.borrow_mut().missing = missing.clone();
        return missing;
    }

    pub fn known(&self, filter: filter::Filter, from: git2::Oid) -> bool {
        self.get2(filter, from).is_some()
    }

    pub fn get(&self, filter: filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        if let Some(x) = self.get2(filter, from) {
            return Some(x);
        } else {
            let mut t2 = self.t2.borrow_mut();
            t2.misses += 1;
            t2.missing.push((filter, from));
            return None;
        }
    }

    fn get2(&self, filter: filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        if filter == filter::nop() {
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

        return None;
    }
}
