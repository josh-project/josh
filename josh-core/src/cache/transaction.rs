use super::history_graph::compute_sequence_number;
use super::sled::sled_open_josh_trees;
use super::stack::CacheStack;
use anyhow::anyhow;

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

pub trait FilterHook {
    fn filter_for_commit(
        &self,
        commit_oid: git2::Oid,
        arg: &str,
    ) -> anyhow::Result<crate::filter::Filter>;
}

static REF_CACHE: LazyLock<RwLock<HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>>> =
    LazyLock::new(Default::default);

static POPULATE_MAP: LazyLock<RwLock<HashMap<(git2::Oid, git2::Oid), git2::Oid>>> =
    LazyLock::new(Default::default);

static GLOB_MAP: LazyLock<RwLock<HashMap<(git2::Oid, git2::Oid), git2::Oid>>> =
    LazyLock::new(Default::default);

/// Clear the process-global in-memory caches shared across all transactions.
pub fn clear_global_caches() {
    REF_CACHE.write().unwrap().clear();
    POPULATE_MAP.write().unwrap().clear();
    GLOB_MAP.write().unwrap().clear();
}

pub struct TransactionContext {
    path: std::path::PathBuf,
    cache: std::sync::Arc<CacheStack>,
    ref_prefix: Option<String>,
    mem_odb_limit: Option<usize>,
    ephemeral: bool,
}

impl TransactionContext {
    pub fn from_env(cache: std::sync::Arc<CacheStack>) -> anyhow::Result<Self> {
        let repo = git2::Repository::open_from_env()?;
        let path = repo.path().to_owned();

        Ok(Self {
            path,
            cache,
            ref_prefix: None,
            mem_odb_limit: None,
            ephemeral: false,
        })
    }

    pub fn new(path: impl AsRef<std::path::Path>, cache: std::sync::Arc<CacheStack>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            cache,
            ref_prefix: None,
            mem_odb_limit: None,
            ephemeral: false,
        }
    }

    pub fn with_ref_prefix(mut self, prefix: impl AsRef<str>) -> Self {
        self.ref_prefix = Some(prefix.as_ref().to_string());
        self
    }

    pub fn with_mem_odb_limit(mut self, limit: usize) -> Self {
        self.mem_odb_limit = Some(limit);
        self
    }

    pub fn ephemeral(mut self) -> Self {
        self.mem_odb_limit = None;
        self.ephemeral = true;
        self
    }

    pub fn open(&self) -> anyhow::Result<Transaction> {
        if !self.path.exists() {
            return Err(anyhow!("path does not exist"));
        }

        Ok(Transaction::new(
            git2::Repository::open_ext(
                &self.path,
                git2::RepositoryOpenFlags::NO_SEARCH,
                &[] as &[&std::ffi::OsStr],
            )?,
            self.cache.clone(),
            self.ref_prefix.as_deref(),
            self.mem_odb_limit,
            self.ephemeral,
        ))
    }
}

#[allow(unused)]
struct Transaction2 {
    commit_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    apply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    subtract_map: HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    intersect_map: HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    overlay_map: HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    unapply_map: HashMap<git2::Oid, HashMap<git2::Oid, git2::Oid>>,
    legalize_map: HashMap<(crate::filter::Filter, git2::Oid), crate::filter::Filter>,
    downstack_deps_map: HashMap<git2::Oid, std::collections::HashSet<crate::filter::DownstackDep>>,
    merge_trees_map: HashMap<(git2::Oid, git2::Oid, git2::Oid), git2::Oid>,

    cache: std::sync::Arc<CacheStack>,
    path_tree: sled::Tree,
    invert_tree: sled::Tree,
    trigram_index_tree: sled::Tree,
    missing: Vec<(usize, crate::filter::Filter, git2::Oid)>,
    misses: usize,
    nesting_level: usize,
}

pub struct Transaction {
    t2: std::cell::RefCell<Transaction2>,
    repo: git2::Repository,
    /// Per-transaction in-memory object store, flushed to a packfile when the transaction drops, at
    /// an explicit boundary, or mid-transaction when it exceeds its size limit. Never shared with
    /// another transaction.
    mem_odb: std::sync::Arc<josh_memodb::MemOdb>,
    mem_odb_limit: Option<usize>,
    ephemeral: bool,
    ref_prefix: Option<String>,
    filter_hook: Option<std::sync::Arc<dyn FilterHook + Send + Sync>>,
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // Skip flushing to disk, the mem odb will be lost, as requested.
        if self.ephemeral {
            return;
        }

        if let Err(e) = self.mem_odb.flush() {
            log::error!("failed to flush in-memory object store: {e}");
        }
    }
}

impl Transaction {
    fn new(
        repo: git2::Repository,
        cache: std::sync::Arc<CacheStack>,
        ref_prefix: Option<&str>,
        mem_odb_limit: Option<usize>,
        ephemeral: bool,
    ) -> Transaction {
        static GIT2_SET_GLOBAL_OPTS: std::sync::Once = std::sync::Once::new();
        GIT2_SET_GLOBAL_OPTS.call_once(|| {
            // Disable libgit2's strict object creation globally: josh only ever writes objects
            // whose referenced objects it has just produced or read, so the per-write existence
            // checks are pure overhead. This is a process-wide C global, set exactly once.
            git2::opts::strict_object_creation(false);

            // Disable libgit2's parsed object cache. The cache has a limited size,
            // and the way the eviction mechanism works when eviction is needed
            // creates frequent cache misses.
            git2::opts::enable_caching(false);
        });

        let mem_odb = josh_memodb::MemOdb::new(mem_odb_limit, repo.path().to_owned());
        mem_odb.register(&repo);

        log::debug!("new transaction");

        let (path_tree, invert_tree, trigram_index_tree) =
            sled_open_josh_trees().expect("failed to open transaction");

        Transaction {
            t2: std::cell::RefCell::new(Transaction2 {
                commit_map: HashMap::new(),
                apply_map: HashMap::new(),
                subtract_map: HashMap::new(),
                intersect_map: HashMap::new(),
                overlay_map: HashMap::new(),
                unapply_map: HashMap::new(),
                legalize_map: HashMap::new(),
                downstack_deps_map: HashMap::new(),
                merge_trees_map: HashMap::new(),
                cache,
                path_tree,
                invert_tree,
                trigram_index_tree,
                missing: vec![],
                misses: 0,
                nesting_level: 0,
            }),
            repo,
            mem_odb,
            mem_odb_limit,
            ephemeral,
            ref_prefix: ref_prefix.map(|prefix| prefix.to_owned()),
            filter_hook: None,
        }
    }

    pub fn try_clone(&self) -> anyhow::Result<Transaction> {
        let context = TransactionContext {
            cache: self.t2.borrow().cache.clone(),
            path: self.repo.path().to_owned(),
            ref_prefix: self.ref_prefix.clone(),
            mem_odb_limit: self.mem_odb_limit,
            ephemeral: self.ephemeral,
        };

        context.open()
    }

    pub fn repo(&self) -> &git2::Repository {
        &self.repo
    }

    // TODO: remove and rework proxy git launch path to use spawn_git
    pub fn flush_mem_odb(&self) -> anyhow::Result<()> {
        self.mem_odb.flush()?;
        Ok(())
    }

    /// Flush this transaction's in-memory objects, then run a `git` subprocess against its repo. Use
    /// this in place of [`crate::git::spawn_git_command`] whenever a transaction is in scope: the
    /// spawned `git` reads objects from disk and cannot see the in-memory backend, so the store must
    /// be flushed first.
    pub fn spawn_git(&self, args: &[&str], env: &[(&str, &str)]) -> anyhow::Result<()> {
        self.flush_mem_odb()?;
        crate::git::spawn_git_command(self.repo.path(), args, env)
    }

    pub fn refname(&self, r: &str) -> String {
        let ref_prefix = self.ref_prefix.as_deref().unwrap_or_default();
        format!("{}{}", ref_prefix, r)
    }

    pub fn misses(&self) -> usize {
        self.t2.borrow().misses
    }

    pub fn set_nesting(&self, level: usize) -> usize {
        let prev = self.t2.borrow().nesting_level;
        self.t2.borrow_mut().nesting_level = level;
        prev
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

    pub(crate) fn insert_downstack_deps(
        &self,
        oid: git2::Oid,
        deps: std::collections::HashSet<crate::filter::DownstackDep>,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.downstack_deps_map.insert(oid, deps);
    }

    pub(crate) fn get_downstack_deps(
        &self,
        oid: git2::Oid,
    ) -> Option<std::collections::HashSet<crate::filter::DownstackDep>> {
        let t2 = self.t2.borrow_mut();
        t2.downstack_deps_map.get(&oid).cloned()
    }

    pub(crate) fn insert_merge_trees(
        &self,
        key: (git2::Oid, git2::Oid, git2::Oid),
        result: git2::Oid,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.merge_trees_map.insert(key, result);
    }

    pub(crate) fn get_merge_trees(
        &self,
        key: (git2::Oid, git2::Oid, git2::Oid),
    ) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        t2.merge_trees_map.get(&key).copied()
    }

    pub fn insert_subtract(&self, from: (git2::Oid, git2::Oid), to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.subtract_map.insert(from, to);
    }

    pub fn get_subtract(&self, from: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        t2.subtract_map.get(&from).cloned()
    }

    pub fn insert_intersect(&self, from: (git2::Oid, git2::Oid), to: git2::Oid) {
        let mut t2 = self.t2.borrow_mut();
        t2.intersect_map.insert(from, to);
    }

    pub fn get_intersect(&self, from: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        let t2 = self.t2.borrow_mut();
        t2.intersect_map.get(&from).cloned()
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
    ) -> anyhow::Result<crate::filter::Filter> {
        if let Some(h) = &self.filter_hook {
            return h.filter_for_commit(from, hook);
        }
        Err(anyhow!("missing filter hook"))
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
    ) -> anyhow::Result<()> {
        let sequence_number = if filter != crate::filter::sequence_number()
            && filter != crate::filter::reachable_roots()
        {
            compute_sequence_number(self, from)?
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
            t2.cache.write_all(filter, from, to, sequence_number)?;
        }
        Ok(())
    }

    pub fn get_missing(&self) -> anyhow::Result<Vec<(usize, crate::filter::Filter, git2::Oid)>> {
        let missing = self.t2.borrow().missing.clone();
        let mut retained = Vec::with_capacity(missing.len());
        for (level, f, i) in missing {
            if !self.known(f, i)? {
                retained.push((level, f, i));
            }
        }
        retained.sort_by_key(|(l, f, i)| (*f, *i, *l));
        retained.dedup_by_key(|(_, f, i)| (*f, *i));
        retained.sort();
        self.t2.borrow_mut().missing = retained.clone();
        Ok(retained)
    }

    pub fn known(&self, filter: crate::filter::Filter, from: git2::Oid) -> anyhow::Result<bool> {
        Ok(self.get2(filter, from)?.is_some())
    }

    pub fn get(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
    ) -> anyhow::Result<Option<git2::Oid>> {
        if let Some(x) = self.get2(filter, from)? {
            Ok(Some(x))
        } else {
            let mut t2 = self.t2.borrow_mut();
            let nesting_level = t2.nesting_level;
            t2.misses += 1;
            t2.missing.push((nesting_level, filter, from));
            Ok(None)
        }
    }

    fn get2(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
    ) -> anyhow::Result<Option<git2::Oid>> {
        if filter.is_nop() {
            return Ok(Some(from));
        }
        let sequence_number = if filter != crate::filter::sequence_number()
            && filter != crate::filter::reachable_roots()
        {
            compute_sequence_number(self, from)?
        } else {
            0
        };
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.commit_map.get(&filter.id())
            && let Some(oid) = m.get(&from).cloned()
        {
            return Ok(Some(oid));
        }

        let oid = t2.cache.read_propagate(filter, from, sequence_number)?;

        if let Some(oid) = oid {
            if oid == git2::Oid::zero() {
                return Ok(Some(oid));
            }
            if filter == crate::filter::sequence_number() {
                return Ok(Some(oid));
            }

            if self.repo.odb()?.exists(oid) {
                // Only report an object as cached if it exists in the object database.
                // This forces a rebuild in case the object was garbage collected.
                return Ok(Some(oid));
            }
        }

        Ok(None)
    }
}
