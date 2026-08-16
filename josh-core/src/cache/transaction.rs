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

/// What [`Transaction::update_ref`] requires the ref to currently be before it is
/// repointed at the new target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// No requirement: overwrite whatever is there.
    Any,
    /// The ref must not exist yet.
    Absent,
    /// The ref must currently point at exactly this oid.
    At(git2::Oid),
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

// Path-projection memoization for `:PATHS` and its inverse, keyed by (input tree oid, root path).
// Both are pure functions of the input tree, and workspace filters walk commits parent-first, so a
// child commit reuses the projections its parent just computed for the subtrees they share, which
// an in-process map captures.
static PATHS_MAP: LazyLock<RwLock<HashMap<(git2::Oid, String), git2::Oid>>> =
    LazyLock::new(Default::default);

static INVERT_MAP: LazyLock<RwLock<HashMap<(git2::Oid, String), git2::Oid>>> =
    LazyLock::new(Default::default);

/// Placeholder hint for tree-keyed records with no commit context. Sequence 0 is
/// eligible and lands in shard 0 of the distributed backend; the local backend
/// ignores the hint.
const TREE_KEYED_FALLBACK_HINT: HistoryGraphHint = HistoryGraphHint {
    sequence_number: 0,
    parent_count: 1,
    jump_delta: 1,
    jump_is_second: false,
};

/// Clear the process-global in-memory caches shared across all transactions.
pub fn clear_global_caches() {
    REF_CACHE.write().unwrap().clear();
    POPULATE_MAP.write().unwrap().clear();
    GLOB_MAP.write().unwrap().clear();
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
    // In-transaction memoization of the trigram index (source tree -> index tree); the
    // cache backend behind it holds the durable, cross-transaction copy.
    index_map: HashMap<git2::Oid, git2::Oid>,
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
        // Balance the `begin` from `Transaction::new`, even for ephemeral transactions, so a
        // locking backend can release once the last transaction ends.
        self.t2.borrow().cache.end();

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

        // Balanced in `Drop`; lets a locking backend (sled) hold its lock only while a
        // transaction is live.
        cache.begin();

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
                index_map: HashMap::new(),
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

    /// Create or update the direct ref `refname` to point at `target`, guarded by
    /// `expected`: with `Expected::Any` an existing ref is overwritten unconditionally
    /// (updating to the value a ref already has is a no-op); `Expected::At(oid)` asserts
    /// the ref currently points at `oid`; `Expected::Absent` asserts it does not exist.
    /// On a failed assertion — including a ref that appeared or disappeared
    /// concurrently — an error is returned and the ref is unchanged. Writers that derive
    /// `target` from the ref's old value pass `At`/`Absent` so they never overwrite a
    /// concurrent update. (gix mapping: `PreviousValue::Any` / `MustExistAndMatch` /
    /// `MustNotExist`.)
    pub fn update_ref(
        &self,
        refname: &str,
        expected: Expected,
        target: git2::Oid,
        log_message: &str,
    ) -> anyhow::Result<()> {
        match expected {
            Expected::Any => {
                self.repo.reference(refname, target, true, log_message)?;
            }
            Expected::At(old) => {
                self.repo
                    .reference_matching(refname, target, true, old, log_message)?;
            }
            Expected::Absent => {
                self.repo.reference(refname, target, false, log_message)?;
            }
        }
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

    /// Build a josh-search index cache that shards records by `commit`'s history-graph
    /// position. Pass [`git2::Oid::ZERO_SHA1`] for a bare tree with no commit context.
    pub fn trigram_index_cache(&self, commit: git2::Oid) -> TrigramIndexCache<'_> {
        TrigramIndexCache {
            transaction: self,
            hint: self.tree_keyed_hint(commit),
        }
    }

    /// Trees carry no history position of their own, so tree-keyed records use the hint
    /// of the commit being indexed. Falls back to the placeholder when there is no commit
    /// context or the hint cannot be computed.
    fn tree_keyed_hint(&self, commit: git2::Oid) -> HistoryGraphHint {
        if commit == git2::Oid::ZERO_SHA1 {
            return TREE_KEYED_FALLBACK_HINT;
        }
        compute_history_hint(self, commit).unwrap_or(TREE_KEYED_FALLBACK_HINT)
    }

    fn insert_trigram_index(&self, tree: git2::Oid, result: git2::Oid, hint: HistoryGraphHint) {
        let filter = crate::filter::index();
        let mut t2 = self.t2.borrow_mut();
        t2.index_map.entry(tree).or_insert(result);
        if let Err(e) = t2.cache.write_all(filter, tree, result, hint, true) {
            log::warn!("trigram index cache write failed: {e}");
        }
    }

    fn get_trigram_index(&self, tree: git2::Oid, hint: HistoryGraphHint) -> Option<git2::Oid> {
        let filter = crate::filter::index();
        let t2 = self.t2.borrow_mut();
        if let Some(oid) = t2.index_map.get(&tree).cloned() {
            // Written this transaction, so the index tree is live in the mem odb -- no odb check.
            return Some(oid);
        }
        let oid = t2.cache.read_propagate(filter, tree, hint, true).ok()??;
        // Per-subtree index trees are anchored by no ref, so gc may have pruned a
        // cached one; treat a dangling hit as a miss and reindex.
        if self.repo.odb().ok()?.exists(oid) {
            Some(oid)
        } else {
            None
        }
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
            t2.cache.write_all(filter, from, to, hint, false)?;
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

        let oid = t2.cache.read_propagate(filter, from, hint, false)?;

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

/// josh-search index memoization backed by the cache backend under the `:INDEX`
/// filter: durable (sled) and shareable (distributed) across processes, keyed by
/// source tree oid. The bound `hint` is the indexed commit's history position,
/// resolved once per commit and reused for every per-subtree lookup of its walk.
pub struct TrigramIndexCache<'a> {
    transaction: &'a Transaction,
    hint: HistoryGraphHint,
}

impl josh_search::IndexCache for TrigramIndexCache<'_> {
    fn get_index(&self, tree: git2::Oid) -> Option<git2::Oid> {
        self.transaction.get_trigram_index(tree, self.hint)
    }

    fn set_index(&self, tree: git2::Oid, index: git2::Oid) {
        self.transaction
            .insert_trigram_index(tree, index, self.hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_transaction() -> (tempfile::TempDir, Transaction) {
        // These tests exercise ref/commit machinery, not the on-disk cache, so an empty cache
        // stack (no sled backend) is enough.
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
            .update_ref("refs/heads/main", Expected::Any, oid, "test")
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
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        transaction
            .update_ref("refs/heads/main", Expected::Any, b, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
        // Re-writing the current value succeeds as a no-op.
        transaction
            .update_ref("refs/heads/main", Expected::Any, b, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
    }

    #[test]
    fn update_ref_absent_creates() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        transaction
            .update_ref("refs/heads/main", Expected::Absent, a, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(a));
    }

    #[test]
    fn update_ref_absent_fails_on_existing_ref() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        assert!(
            transaction
                .update_ref("refs/heads/main", Expected::Absent, b, "test")
                .is_err()
        );
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(a));
    }

    #[test]
    fn update_ref_at_updates_on_match() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        transaction
            .update_ref("refs/heads/main", Expected::At(a), b, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
    }

    #[test]
    fn update_ref_at_fails_on_mismatch() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        let c = commit(&transaction, "c");
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        assert!(
            transaction
                .update_ref("refs/heads/main", Expected::At(b), c, "test")
                .is_err()
        );
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(a));
    }

    #[test]
    fn update_ref_at_fails_on_missing_ref() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        assert!(
            transaction
                .update_ref("refs/heads/missing", Expected::At(a), b, "test")
                .is_err()
        );
        assert_eq!(transaction.resolve_ref("refs/heads/missing").unwrap(), None);
    }

    #[test]
    fn update_ref_at_matches_packed_ref() {
        let (dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/josh/a", Expected::Any, a, "test")
            .unwrap();
        // Pack the ref by hand (git2 exposes no pack-refs API): the CAS must see the
        // packed value, not conclude the ref is absent.
        std::fs::write(
            dir.path().join("packed-refs"),
            format!(
                "# pack-refs with: peeled fully-peeled sorted\n{} refs/josh/a\n",
                a
            ),
        )
        .unwrap();
        std::fs::remove_file(dir.path().join("refs/josh/a")).unwrap();

        transaction
            .update_ref("refs/josh/a", Expected::At(a), b, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/a").unwrap(), Some(b));
    }

    #[test]
    fn for_each_ref_prefixed_is_sorted_and_skips_symbolic() {
        let (dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        // Insertion order deliberately unsorted; refs/josh-x/ must not match the
        // refs/josh/ prefix; the symbolic ref sorts inside the range but is skipped.
        transaction
            .update_ref("refs/josh/b", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .update_ref("refs/josh/a", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .update_ref("refs/josh-x/c", Expected::Any, oid, "test")
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
        transaction
            .update_ref("refs/josh/a", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .update_ref("refs/josh/b", Expected::Any, oid, "test")
            .unwrap();

        let mut calls = 0;
        let result = transaction.for_each_ref_prefixed("refs/josh/", |_, _| {
            calls += 1;
            Err(anyhow::anyhow!("stop"))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}
