use super::sled::sled_open_josh_trees;
use super::stack::CacheStack;

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

pub(crate) const CACHE_VERSION: u64 = 25;

pub trait CacheBackend: Send + Sync {
    fn read(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        sequence_number: u128,
    ) -> crate::JoshResult<Option<git2::Oid>>;

    fn write(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
        sequence_number: u128,
    ) -> crate::JoshResult<()>;
}

pub trait FilterHook {
    fn filter_for_commit(
        &self,
        commit_oid: git2::Oid,
        arg: &str,
    ) -> crate::JoshResult<crate::filter::Filter>;
}

pub(crate) fn josh_commit_signature<'a>() -> crate::JoshResult<git2::Signature<'a>> {
    Ok(if let Ok(time) = std::env::var("JOSH_COMMIT_TIME") {
        git2::Signature::new(
            "JOSH",
            "josh@josh-project.dev",
            &git2::Time::new(time.parse()?, 0),
        )?
    } else {
        git2::Signature::now("JOSH", "josh@josh-project.dev")?
    })
}

static REF_CACHE: LazyLock<RwLock<HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>>> =
    LazyLock::new(Default::default);

static POPULATE_MAP: LazyLock<RwLock<HashMap<(git2::Oid, git2::Oid), git2::Oid>>> =
    LazyLock::new(Default::default);

static GLOB_MAP: LazyLock<RwLock<HashMap<(git2::Oid, git2::Oid), git2::Oid>>> =
    LazyLock::new(Default::default);

pub struct TransactionContext {
    path: std::path::PathBuf,
    cache: std::sync::Arc<CacheStack>,
}

impl TransactionContext {
    pub fn from_env(cache: std::sync::Arc<CacheStack>) -> crate::JoshResult<Self> {
        let repo = git2::Repository::open_from_env()?;
        let path = repo.path().to_owned();

        Ok(Self { path, cache })
    }

    pub fn new(path: impl AsRef<std::path::Path>, cache: std::sync::Arc<CacheStack>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            cache,
        }
    }

    pub fn open(&self, ref_prefix: Option<&str>) -> crate::JoshResult<Transaction> {
        if !self.path.exists() {
            return Err(crate::josh_error("path does not exist"));
        }

        Ok(Transaction::new(
            git2::Repository::open_ext(
                &self.path,
                git2::RepositoryOpenFlags::NO_SEARCH,
                &[] as &[&std::ffi::OsStr],
            )?,
            self.cache.clone(),
            ref_prefix,
        ))
    }
}

#[allow(unused)]
struct Transaction2 {
    commit_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    apply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    subtract_map: HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    overlay_map: HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    unapply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    legalize_map: HashMap<(crate::filter::Filter, git2::Oid), crate::filter::Filter>,

    cache: std::sync::Arc<CacheStack>,
    path_tree: sled::Tree,
    invert_tree: sled::Tree,
    trigram_index_tree: sled::Tree,
    missing: Vec<(crate::filter::Filter, git2::Oid)>,
    misses: usize,
    walks: usize,
}

pub struct Transaction {
    t2: std::cell::RefCell<Transaction2>,
    repo: git2::Repository,
    ref_prefix: String,
    filter_hook: Option<std::sync::Arc<dyn FilterHook + Send + Sync>>,
}

impl Transaction {
    fn new(
        repo: git2::Repository,
        cache: std::sync::Arc<CacheStack>,
        ref_prefix: Option<&str>,
    ) -> Transaction {
        log::debug!("new transaction");

        let (path_tree, invert_tree, trigram_index_tree) =
            sled_open_josh_trees().expect("failed to open transaction");

        Transaction {
            t2: std::cell::RefCell::new(Transaction2 {
                commit_map: HashMap::new(),
                apply_map: HashMap::new(),
                subtract_map: HashMap::new(),
                overlay_map: HashMap::new(),
                unapply_map: HashMap::new(),
                legalize_map: HashMap::new(),
                cache,
                path_tree,
                invert_tree,
                trigram_index_tree,
                missing: vec![],
                misses: 0,
                walks: 0,
            }),
            repo,
            ref_prefix: ref_prefix.unwrap_or("").to_string(),
            filter_hook: None,
        }
    }

    pub fn try_clone(&self) -> crate::JoshResult<Transaction> {
        let context = TransactionContext {
            cache: self.t2.borrow().cache.clone(),
            path: self.repo.path().to_owned(),
        };

        context.open(Some(&self.ref_prefix))
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
        prev
    }

    pub fn end_walk(&self) {
        self.t2.borrow_mut().walks -= 1;
    }

    pub fn insert_apply(&self, filter: crate::filter::Filter, from: git2::Oid, to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.apply_map
            .entry(filter.id())
            .or_default()
            .insert(from, to);
    }

    pub fn get_apply(&self, filter: crate::filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.apply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        None
    }

    pub fn insert_subtract(&self, from: (git2::Oid, git2::Oid), to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.subtract_map.insert(from, to);
    }

    pub fn get_subtract(&self, from: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        t2.subtract_map.get(&from).cloned()
    }

    pub fn insert_overlay(&self, from: (git2::Oid, git2::Oid), to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.overlay_map.insert(from, to);
    }

    pub fn get_overlay(&self, from: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        t2.overlay_map.get(&from).cloned()
    }

    pub fn insert_legalize(
        &self,
        from: (crate::filter::Filter, git2::Oid),
        to: crate::filter::Filter,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.legalize_map.insert(from, to);
    }

    pub fn get_legalize(
        &self,
        from: (crate::filter::Filter, git2::Oid),
    ) -> Option<crate::filter::Filter> {
        let t2 = self.t2.borrow_mut();
        t2.legalize_map.get(&from).cloned()
    }

    pub fn insert_unapply(&self, filter: crate::filter::Filter, from: git2::Oid, to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.unapply_map
            .entry(filter.id())
            .or_default()
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
        None
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
        None
    }

    pub fn insert_trigram_index(&self, tree: git2::Oid, result: git2::Oid) {
        let t2 = self.t2.borrow();
        t2.trigram_index_tree
            .insert(tree.as_bytes(), result.as_bytes())
            .unwrap();
    }

    pub fn get_trigram_index(&self, tree: git2::Oid) -> Option<git2::Oid> {
        let t2 = self.t2.borrow();

        if let Some(oid) = t2.trigram_index_tree.get(tree.as_bytes()).unwrap() {
            return Some(git2::Oid::from_bytes(&oid).unwrap());
        }
        None
    }

    pub fn insert_populate(&self, tree: (git2::Oid, git2::Oid), result: git2::Oid) {
        POPULATE_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_populate(&self, tree: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        POPULATE_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_glob(&self, tree: (git2::Oid, git2::Oid), result: git2::Oid) {
        GLOB_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_glob(&self, tree: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        GLOB_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_ref(&self, filter: crate::filter::Filter, from: git2::Oid, to: git2::Oid) {
        REF_CACHE
            .write()
            .unwrap()
            .entry(filter.id())
            .or_default()
            .insert(from, to);
    }

    pub fn get_ref(&self, filter: crate::filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        if let Some(m) = REF_CACHE.read().unwrap().get(&filter.id())
            && let Some(oid) = m.get(&from)
            && self.repo.odb().unwrap().exists(*oid)
        {
            return Some(*oid);
        }
        None
    }

    pub fn get_unapply(&self, filter: crate::filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.unapply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        None
    }

    pub fn lookup_filter_hook(
        &self,
        hook: &str,
        from: git2::Oid,
    ) -> crate::JoshResult<crate::filter::Filter> {
        if let Some(h) = &self.filter_hook {
            return h.filter_for_commit(from, hook);
        }
        Err(crate::josh_error("missing filter hook"))
    }

    pub fn with_filter_hook(mut self, hook: std::sync::Arc<dyn FilterHook + Send + Sync>) -> Self {
        self.filter_hook = Some(hook);
        self
    }

    pub fn insert(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
        store: bool,
    ) {
        let sequence_number = if filter != crate::filter::sequence_number() {
            compute_sequence_number(self, from).expect("compute_sequence_number failed")
        } else {
            0
        };
        let mut t2 = self.t2.borrow_mut();
        t2.commit_map
            .entry(filter.id())
            .or_default()
            .insert(from, to);

        // In addition to commits that are explicitly requested to be stored, also store
        // random extra commits (probability 1/256) to avoid long searches for filters that reduce
        // the history length by a very large factor.
        if store || from.as_bytes()[0] == 0 {
            t2.cache
                .write_all(filter, from, to, sequence_number)
                .expect("Failed to write cache");
        }
    }

    pub fn get_missing(&self) -> Vec<(crate::filter::Filter, git2::Oid)> {
        let mut missing = self.t2.borrow().missing.clone();
        missing.dedup();
        missing.retain(|(f, i)| !self.known(*f, *i));
        self.t2.borrow_mut().missing = missing.clone();
        missing
    }

    pub fn known(&self, filter: crate::filter::Filter, from: git2::Oid) -> bool {
        self.get2(filter, from).is_some()
    }

    pub fn get(&self, filter: crate::filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        if let Some(x) = self.get2(filter, from) {
            Some(x)
        } else {
            let mut t2 = self.t2.borrow_mut();
            t2.misses += 1;
            t2.missing.push((filter, from));
            None
        }
    }

    fn get2(&self, filter: crate::filter::Filter, from: git2::Oid) -> Option<git2::Oid> {
        if filter.is_nop() {
            return Some(from);
        }
        let sequence_number = if filter != crate::filter::sequence_number() {
            compute_sequence_number(self, from).expect("compute_sequence_number failed")
        } else {
            0
        };
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.commit_map.get(&filter.id())
            && let Some(oid) = m.get(&from).cloned()
        {
            return Some(oid);
        }

        let oid = t2
            .cache
            .read_propagate(filter, from, sequence_number)
            .expect("Failed to read from cache backend");

        if let Some(oid) = oid {
            if oid == git2::Oid::zero() {
                return Some(oid);
            }
            if filter == crate::filter::sequence_number() {
                return Some(oid);
            }

            if self.repo.odb().unwrap().exists(oid) {
                // Only report an object as cached if it exists in the object database.
                // This forces a rebuild in case the object was garbage collected.
                return Some(oid);
            }
        }

        None
    }
}

/// Encode a `u128` into a 20-byte git OID (SHA-1 sized).
/// The high 4 bytes of the OID are zero; the low 16 bytes
/// contain the big-endian integer.
pub fn oid_from_u128(n: u128) -> git2::Oid {
    let mut bytes = [0u8; 20];
    // place the 16 integer bytes at the end (big-endian)
    bytes[20 - 16..].copy_from_slice(&n.to_be_bytes());
    // Safe: length is exactly 20
    git2::Oid::from_bytes(&bytes).expect("20-byte OID construction cannot fail")
}

/// Decode a `u128` previously encoded by `oid_from_u128`.
pub fn u128_from_oid(oid: git2::Oid) -> u128 {
    let b = oid.as_bytes();
    let mut n = [0u8; 16];
    n.copy_from_slice(&b[20 - 16..]); // take the last 16 bytes
    u128::from_be_bytes(n)
}

/// Computes the sequence number for each commit so that the sequence_number for any
/// commit is always larger than the sequence_number of all of its parents.
/// This means sorting by sequence number results in a topological order.
pub fn compute_sequence_number(
    transaction: &Transaction,
    input: git2::Oid,
) -> crate::JoshResult<u128> {
    if let Some(count) = transaction.get(crate::filter::sequence_number(), input) {
        return Ok(u128_from_oid(count));
    }

    if !transaction.repo().odb()?.exists(input) {
        return Err(crate::josh_error(
            "compute_sequence_number: input does not exist",
        ));
    }

    let commit = transaction.repo().find_commit(input)?;
    let mut this_sequence_number = 0;
    let mut no_walk = true;
    for parent in commit.parent_ids() {
        if let Some(parent_sequence_number) =
            transaction.get(crate::filter::sequence_number(), parent)
        {
            let parent_sequence_number = u128_from_oid(parent_sequence_number);
            this_sequence_number = std::cmp::max(this_sequence_number, parent_sequence_number + 1);
        } else {
            no_walk = false;
            break;
        }
    }

    if no_walk {
        transaction.insert(
            crate::filter::sequence_number(),
            commit.id(),
            oid_from_u128(this_sequence_number),
            true,
        );
    } else {
        log::info!("compute_sequence_number: new_walk for {:?}", input);
        let mut walk = transaction.repo().revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(input)?;

        for c in walk {
            let commit = transaction.repo().find_commit(c?)?;
            let mut this_sequence_number = 0;
            for parent in commit.parent_ids() {
                let parent_sequence_number = compute_sequence_number(transaction, parent)?;
                this_sequence_number =
                    std::cmp::max(this_sequence_number, parent_sequence_number + 1);
            }
            transaction.insert(
                crate::filter::sequence_number(),
                commit.id(),
                oid_from_u128(this_sequence_number),
                true,
            );
        }
    }
    if let Some(count) = transaction.get(crate::filter::sequence_number(), input) {
        Ok(u128_from_oid(count))
    } else {
        Err(crate::josh_error("missing sequence_number"))
    }
}
