use super::backend::HistoryGraphHint;
use super::history_graph::compute_history_hint;
use super::stack::CacheStack;
use super::tree_cache::{TreeBytes, TreeCache};
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

// Keyed by (input tree, pattern key, NFA state mask). The state mask makes entries independent
// of the path a subtree was reached through; the legacy full-path fallback folds its root path
// into a synthetic pattern key and uses mask 0.
static GLOB_MAP: LazyLock<RwLock<HashMap<(git2::Oid, git2::Oid, u64), git2::Oid>>> =
    LazyLock::new(Default::default);

// Trigram index memoization, keyed by source tree oid -> index tree oid. The index is a pure
// function of the source tree, and `:INDEX` walks commits parent-first, so when a child commit is
// indexed the subtrees it shares with its parent are already memoized here from the parent's run --
// which is where reuse almost always comes from. An in-process map captures that without the
// persistence (and file lock) of an on-disk cache.
static TRIGRAM_INDEX_MAP: LazyLock<RwLock<HashMap<git2::Oid, git2::Oid>>> =
    LazyLock::new(Default::default);

// Path-projection memoization for `:PATHS` and its inverse, keyed by (input tree oid, root path).
// Both are pure functions of the input tree, and workspace filters walk commits parent-first, so a
// child commit reuses the projections its parent just computed for the subtrees they share -- the
// same parent-reuse the trigram index relies on, so an in-process map suffices here too.
static PATHS_MAP: LazyLock<RwLock<HashMap<(git2::Oid, String), git2::Oid>>> =
    LazyLock::new(Default::default);

static INVERT_MAP: LazyLock<RwLock<HashMap<(git2::Oid, String), git2::Oid>>> =
    LazyLock::new(Default::default);

/// Clear the process-global in-memory caches shared across all transactions.
pub fn clear_global_caches() {
    REF_CACHE.write().unwrap().clear();
    POPULATE_MAP.write().unwrap().clear();
    GLOB_MAP.write().unwrap().clear();
    TRIGRAM_INDEX_MAP.write().unwrap().clear();
    PATHS_MAP.write().unwrap().clear();
    INVERT_MAP.write().unwrap().clear();
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
    last_written_commit: Option<(git2::Oid, git2::Oid)>,
    tree_cache: TreeCache,

    cache: std::sync::Arc<CacheStack>,
    missing: Vec<(usize, crate::filter::Filter, git2::Oid)>,
    misses: usize,
    nesting_level: usize,
}

pub struct Transaction {
    t2: std::cell::RefCell<Transaction2>,
    /// josh-search indexing state, kept for the whole transaction so indexing a chain of
    /// commits reuses the per-blob/per-directory merge work of earlier commits. Its own cell:
    /// it stays borrowed across a whole `trigram_index` call, which reads other caches through
    /// `t2`.
    trigram_indexer: std::cell::RefCell<josh_search::Indexer>,
    /// josh-search search memoization, kept for the whole transaction so searching many
    /// commits (e.g. one GraphQL query over `history { search }`) reuses candidate walks of
    /// shared subtrees and verifies each distinct blob once.
    search_cache: std::cell::RefCell<josh_search::SearchCache>,
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
        // Turn off libgit2's strictness checks. These are process-wide C globals, set
        // exactly once.
        static GIT2_OPTIONS: std::sync::Once = std::sync::Once::new();
        GIT2_OPTIONS.call_once(|| {
            // Don't check per write that referenced objects exist: josh only ever writes
            // objects whose referenced objects it has just produced or read, so the checks
            // are pure overhead.
            git2::opts::strict_object_creation(false);
            // Don't re-hash objects on every read: libgit2 defaults to verifying each
            // object against its id with collision-detecting SHA1 on every lookup. josh
            // only reads objects it wrote itself or that git verified on transfer, so this
            // costs a full hash pass per object read and buys nothing.
            git2::opts::strict_hash_verification(false);
        });

        let mem_odb = josh_memodb::MemOdb::new(mem_odb_limit, repo.path().to_owned());
        mem_odb.register(&repo);

        log::debug!("new transaction");

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
                last_written_commit: None,
                tree_cache: Default::default(),
                cache,
                missing: vec![],
                misses: 0,
                nesting_level: 0,
            }),
            trigram_indexer: Default::default(),
            search_cache: Default::default(),
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

    /// Close the on-disk cache, releasing sled's exclusive lock on the cache directory so other
    /// josh processes can open it. This flushes pending writes, then drops the remaining sled
    /// handles: the shared backend's per-filter trees and the process-global database.
    ///
    /// Call once no further cache reads or writes will happen. The repo and in-memory object store
    /// stay usable, so a filtering phase can hand its result to a long, cache-free tail (for
    /// example running containers) without pinning the lock. A later cache read or write reopens
    /// what it needs.
    pub fn release_cache(&self) -> anyhow::Result<()> {
        crate::cache::sled::sled_flush()?;
        self.t2.borrow().cache.release();
        crate::cache::sled::sled_unload();
        Ok(())
    }

    /// Read the raw bytes of the tree `oid` through the per-transaction [`TreeCache`]
    /// (see there for the promotion and eviction policy). `Ok(None)` means the object exists
    /// but is not a tree; a missing object is an error, like a plain odb read.
    pub fn read_tree_bytes<'a>(
        &self,
        odb: &'a git2::Odb,
        oid: git2::Oid,
    ) -> anyhow::Result<Option<TreeBytes<'a>>> {
        if let Some(bytes) = self.t2.borrow().tree_cache.get(oid) {
            return Ok(Some(TreeBytes::Cached(bytes)));
        }
        let obj = odb.read(oid)?;
        if obj.kind() != git2::ObjectType::Tree {
            return Ok(None);
        }
        let mut t2 = self.t2.borrow_mut();
        if t2.tree_cache.should_promote(oid) {
            let bytes: std::sync::Arc<[u8]> = obj.data().into();
            t2.tree_cache.insert(oid, bytes.clone());
            return Ok(Some(TreeBytes::Cached(bytes)));
        }
        Ok(Some(TreeBytes::Odb(obj)))
    }

    // TODO: remove and rework proxy git launch path to use spawn_git
    pub fn flush_mem_odb(&self) -> anyhow::Result<()> {
        self.mem_odb.flush()?;
        Ok(())
    }

    /// Flush this transaction's in-memory objects, then build a `git` subprocess against its repo.
    /// Use this in place of [`crate::git::GitCommand::new`] whenever a transaction is in scope: the
    /// spawned `git` reads objects from disk and cannot see the in-memory backend, so the store must
    /// be flushed first.
    pub fn git_command(
        &self,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> anyhow::Result<crate::git::GitCommand> {
        self.flush_mem_odb()?;
        Ok(crate::git::GitCommand::new(
            self.repo.path(),
            args,
            env.iter().copied(),
        ))
    }

    /// Run a `git` subprocess with default stdio handling. See [`Transaction::git_command`].
    pub fn spawn_git(&self, args: &[&str], env: &[(&str, &str)]) -> anyhow::Result<()> {
        self.git_command(args, env)?.spawn().map(|_| ())
    }

    pub fn refname(&self, r: &str) -> String {
        let ref_prefix = self.ref_prefix.as_deref().unwrap_or_default();
        format!("{}{}", ref_prefix, r)
    }

    /// Resolve a fully qualified refname to its target oid, following symbolic refs. No
    /// partial-name DWIM. `Ok(None)` if the ref (or the end of a symbolic chain) does not
    /// exist. The target is not peeled: for an annotated tag ref this is the tag oid,
    /// peeling is an object-store concern.
    pub fn resolve_ref(&self, refname: &str) -> anyhow::Result<Option<git2::Oid>> {
        match self.repo.refname_to_id(refname) {
            Ok(oid) => Ok(Some(oid)),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Force-create or update a direct ref to point at `target`. An existing ref is
    /// overwritten unconditionally; updating to the value a ref already has is a no-op.
    pub fn update_ref(
        &self,
        refname: &str,
        target: git2::Oid,
        log_message: &str,
    ) -> anyhow::Result<()> {
        self.repo.reference(refname, target, true, log_message)?;
        Ok(())
    }

    /// Run `cb` for every direct ref whose full name starts with `prefix`, byte-sorted by
    /// name. `prefix` must consist of refname-valid characters. Symbolic refs and refs
    /// with non-UTF-8 names are skipped. Errors from `cb` abort the iteration.
    pub fn for_each_ref_prefixed(
        &self,
        prefix: &str,
        mut cb: impl FnMut(&str, git2::Oid) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        // Glob metacharacters (*?[\) are all invalid in refnames, so appending `*` — which
        // matches across `/` — turns the glob walk into plain prefix iteration.
        debug_assert!(
            !prefix.contains(['*', '?', '[', '\\']),
            "prefix must consist of refname-valid characters"
        );
        let mut refs = vec![];
        for reference in self.repo.references_glob(&format!("{}*", prefix))? {
            let reference = reference?;
            if let (Ok(name), Some(target)) = (reference.name(), reference.target()) {
                refs.push((name.to_owned(), target));
            }
        }
        // git2 yields loose refs in filesystem order followed by packed ones; sorting makes
        // the order part of the API contract instead of an artifact of the backend.
        refs.sort();
        for (name, target) in refs {
            cb(&name, target)?;
        }
        Ok(())
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

    /// Remember the most recent commit josh wrote in this transaction as `(oid, tree_id)`. A history
    /// walk processes a commit right after writing its parent, so this single slot answers the
    /// common "what tree does my filtered parent have" lookup without re-parsing the parent from the
    /// odb -- and without retaining every written commit the way a map would.
    pub fn set_last_written_commit(&self, commit: git2::Oid, tree: git2::Oid) {
        self.t2.borrow_mut().last_written_commit = Some((commit, tree));
    }

    pub fn last_written_commit(&self) -> Option<(git2::Oid, git2::Oid)> {
        self.t2.borrow().last_written_commit
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
        PATHS_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_paths(&self, tree: (git2::Oid, String)) -> Option<git2::Oid> {
        PATHS_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_invert(&self, tree: (git2::Oid, String), result: git2::Oid) {
        INVERT_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_invert(&self, tree: (git2::Oid, String)) -> Option<git2::Oid> {
        INVERT_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_trigram_index(&self, tree: git2::Oid, result: git2::Oid) {
        TRIGRAM_INDEX_MAP
            .write()
            .unwrap()
            .entry(tree)
            .or_insert(result);
    }

    pub fn get_trigram_index(&self, tree: git2::Oid) -> Option<git2::Oid> {
        let oid = TRIGRAM_INDEX_MAP.read().unwrap().get(&tree).cloned()?;
        // Only report an index as cached if it still exists in the object database: index
        // trees not anchored by a ref (all per-subtree ones) are pruned by gc/repack, and
        // a dangling hit must rebuild instead of erroring.
        if self.repo.odb().ok()?.exists(oid) {
            return Some(oid);
        }
        None
    }

    pub fn insert_populate(&self, tree: (git2::Oid, git2::Oid), result: git2::Oid) {
        POPULATE_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_populate(&self, tree: (git2::Oid, git2::Oid)) -> Option<git2::Oid> {
        POPULATE_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_glob(&self, tree: (git2::Oid, git2::Oid, u64), result: git2::Oid) {
        GLOB_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_glob(&self, tree: (git2::Oid, git2::Oid, u64)) -> Option<git2::Oid> {
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
        let hint = if filter != crate::filter::sequence_number()
            && filter != crate::filter::reachable_roots()
        {
            compute_history_hint(self, from)?
        } else {
            HistoryGraphHint {
                sequence_number: 0,
                parent_count: 1,
                jump_delta: 1,
                jump_is_second: false,
            }
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
            t2.cache.write_all(filter, from, to, hint)?;
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
        let hint = if filter != crate::filter::sequence_number()
            && filter != crate::filter::reachable_roots()
        {
            compute_history_hint(self, from)?
        } else {
            HistoryGraphHint {
                sequence_number: 0,
                parent_count: 1,
                jump_delta: 1,
                jump_is_second: false,
            }
        };
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.commit_map.get(&filter.id())
            && let Some(oid) = m.get(&from).cloned()
        {
            return Ok(Some(oid));
        }

        let oid = t2.cache.read_propagate(filter, from, hint)?;

        if let Some(oid) = oid {
            if oid == git2::Oid::ZERO_SHA1 {
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

impl Transaction {
    /// The transaction-lifetime josh-search indexing state, for passing to
    /// `josh_search::trigram_index` alongside the transaction itself (the [`IndexCache`]).
    pub fn trigram_indexer(&self) -> std::cell::RefMut<'_, josh_search::Indexer> {
        self.trigram_indexer.borrow_mut()
    }

    /// The transaction-lifetime josh-search search memoization, for passing to
    /// `josh_search::search_candidates` / `search_matches`.
    pub fn search_cache(&self) -> std::cell::RefMut<'_, josh_search::SearchCache> {
        self.search_cache.borrow_mut()
    }
}

/// Back josh-search's index memoization with the process-global trigram map, so `:INDEX` (and any
/// other in-transaction indexing) stays incremental across transactions within a process -- keyed
/// by source tree oid, so a child commit reuses the subtrees its parent just indexed.
impl josh_search::IndexCache for Transaction {
    fn get_index(&self, tree: git2::Oid) -> Option<git2::Oid> {
        self.get_trigram_index(tree)
    }

    fn set_index(&self, tree: git2::Oid, index: git2::Oid) {
        self.insert_trigram_index(tree, index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_transaction() -> (tempfile::TempDir, Transaction) {
        // The sled cache is a process-global, so it gets one directory for the whole test
        // binary rather than one per transaction.
        static SLED_DIR: std::sync::LazyLock<tempfile::TempDir> = std::sync::LazyLock::new(|| {
            let dir = tempfile::tempdir().unwrap();
            crate::cache::sled_load(dir.path()).unwrap();
            dir
        });
        let _ = &*SLED_DIR;

        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(dir.path()).unwrap();
        let context = TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(crate::cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        (dir, transaction)
    }

    fn commit(transaction: &Transaction, msg: &str) -> git2::Oid {
        let repo = transaction.repo();
        let tree = repo
            .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        let sig = git2::Signature::new("t", "t@example.com", &git2::Time::new(0, 0)).unwrap();
        repo.commit(None, &sig, &sig, msg, &tree, &[]).unwrap()
    }

    #[test]
    fn resolve_ref_missing_is_none() {
        let (_dir, transaction) = test_transaction();
        assert_eq!(transaction.resolve_ref("refs/heads/missing").unwrap(), None);
    }

    #[test]
    fn resolve_ref_follows_symbolic_chain() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/heads/main", oid, "test")
            .unwrap();
        transaction
            .repo()
            .reference_symbolic("refs/josh/sym", "refs/heads/main", true, "test")
            .unwrap();

        assert_eq!(
            transaction.resolve_ref("refs/heads/main").unwrap(),
            Some(oid)
        );
        assert_eq!(transaction.resolve_ref("refs/josh/sym").unwrap(), Some(oid));
    }

    #[test]
    fn resolve_ref_dangling_symref_is_none() {
        let (_dir, transaction) = test_transaction();
        transaction
            .repo()
            .reference_symbolic("refs/josh/dangling", "refs/heads/missing", true, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/dangling").unwrap(), None);
    }

    #[test]
    fn update_ref_overwrites_unconditionally() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/main", a, "test")
            .unwrap();
        transaction
            .update_ref("refs/heads/main", b, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
        // Re-writing the current value succeeds as a no-op.
        transaction
            .update_ref("refs/heads/main", b, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
    }

    #[test]
    fn for_each_ref_prefixed_is_sorted_and_skips_symbolic() {
        let (dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        // Insertion order deliberately unsorted; refs/josh-x/ must not match the
        // refs/josh/ prefix; the symbolic ref sorts inside the range but is skipped.
        transaction.update_ref("refs/josh/b", oid, "test").unwrap();
        transaction.update_ref("refs/josh/a", oid, "test").unwrap();
        transaction
            .update_ref("refs/josh-x/c", oid, "test")
            .unwrap();
        transaction
            .repo()
            .reference_symbolic("refs/josh/aa", "refs/josh/a", true, "test")
            .unwrap();

        // Pack `refs/josh/a` by hand (git2 exposes no pack-refs API). Loose refs alone come
        // pre-sorted out of the filesystem walk; only a packed ref sorting before a loose
        // one catches removal of the explicit sort.
        std::fs::write(
            dir.path().join("packed-refs"),
            format!(
                "# pack-refs with: peeled fully-peeled sorted\n{} refs/josh/a\n",
                oid
            ),
        )
        .unwrap();
        std::fs::remove_file(dir.path().join("refs/josh/a")).unwrap();

        let mut seen = vec![];
        transaction
            .for_each_ref_prefixed("refs/josh/", |name, target| {
                assert_eq!(target, oid);
                seen.push(name.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, ["refs/josh/a", "refs/josh/b"]);
    }

    #[test]
    fn for_each_ref_prefixed_propagates_callback_errors() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction.update_ref("refs/josh/a", oid, "test").unwrap();
        transaction.update_ref("refs/josh/b", oid, "test").unwrap();

        let mut calls = 0;
        let result = transaction.for_each_ref_prefixed("refs/josh/", |_, _| {
            calls += 1;
            Err(anyhow::anyhow!("stop"))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}
