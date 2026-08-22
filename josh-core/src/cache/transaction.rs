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

/// Where a repository's HEAD points, as [`Transaction::head`] reports it.
pub struct Head {
    /// The ref HEAD resolves to: a fully qualified `refs/heads/...` on a branch, and `HEAD`
    /// itself when detached. Either way, this is the ref to update to move HEAD.
    pub reference: String,
    /// The unpeeled target of [`Head::reference`], for guarding an update against it.
    pub target: git2::Oid,
    /// The commit HEAD resolves to, annotated tags peeled.
    pub commit: git2::Oid,
}

impl Head {
    /// The branch HEAD is on, fully qualified; `None` when HEAD is detached.
    pub fn branch(&self) -> Option<&str> {
        self.reference
            .starts_with("refs/heads/")
            .then_some(&*self.reference)
    }

    /// [`Head::branch`] without its `refs/heads/` prefix, for the places that show it to a
    /// user or match it against a remote's branch names.
    pub fn short_branch(&self) -> Option<&str> {
        self.branch()
            .map(|name| name.strip_prefix("refs/heads/").unwrap_or(name))
    }
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

#[derive(Debug, Clone)]
struct PendingRef {
    name: String,
    change: PendingRefChange,
}

#[derive(Debug, Clone)]
enum PendingRefChange {
    Update {
        expected: Expected,
        target: git2::Oid,
        log_message: String,
    },
    Delete {
        expected: Expected,
    },
    Symbolic {
        target: String,
        log_message: String,
    },
}

#[derive(Debug, Clone)]
enum PendingTarget {
    Object(git2::Oid),
    Symbolic(String),
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

/// The process's opened gitoxide repositories, keyed by repository path. Opening reads
/// configuration and resolves alternates, and the pack indices a store discovers are worth
/// keeping across transactions -- the proxy builds a context per request, which would pay for
/// both every time. Sharing is safe across transactions: a store that misses an object
/// rescans its objects directory once all its known indices come up empty (gix's default
/// refresh mode), so a pack another transaction has just flushed is still found, and ref reads
/// see the loose and packed files as they are. Entries live for the process; the proxy has a
/// fixed pair of repositories and other processes see few paths each.
static GIX_REPOS: LazyLock<
    std::sync::RwLock<HashMap<std::path::PathBuf, std::sync::Arc<gix::ThreadSafeRepository>>>,
> = LazyLock::new(Default::default);

fn shared_gix_repo(
    path: &std::path::Path,
) -> anyhow::Result<std::sync::Arc<gix::ThreadSafeRepository>> {
    if let Some(repo) = GIX_REPOS.read().unwrap().get(path) {
        return Ok(repo.clone());
    }

    let mut repos = GIX_REPOS.write().unwrap();
    if let Some(repo) = repos.get(path) {
        return Ok(repo.clone());
    }

    // Isolated: the repository's own configuration only, no environment or system-wide
    // overrides, so a transaction resolves the same objects and refs whatever the process it
    // runs in.
    let repo = std::sync::Arc::new(gix::ThreadSafeRepository::open_opts(
        path,
        gix::open::Options::isolated(),
    )?);
    repos.insert(path.to_owned(), repo.clone());

    Ok(repo)
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

        let gix_repo = shared_gix_repo(&self.path)?;

        Ok(Transaction::new(
            git2::Repository::open_ext(
                &self.path,
                git2::RepositoryOpenFlags::NO_SEARCH,
                &[] as &[&std::ffi::OsStr],
            )?,
            gix_repo,
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
    /// josh-search indexing state kept for the whole transaction, so indexing a chain of
    /// commits reuses the merge work of earlier commits. Its own cell because `trigram_index`
    /// borrows it for the entire call while also borrowing other caches through `t2`.
    trigram_indexer: std::cell::RefCell<josh_search::Indexer>,
    repo: git2::Repository,
    /// The gitoxide view of the same repository, and the one josh reads objects and refs
    /// through as each of those moves off libgit2. Held in its thread-safe form: a
    /// transaction crosses threads (josh-graphql requires `Send`) while a `gix::Repository`
    /// holds `Rc` snapshots of packed-refs and shallow state and does not.
    gix_repo: std::sync::Arc<gix::ThreadSafeRepository>,
    /// The in-memory object store of this transaction's repository, shared with every other
    /// transaction on the same objects directory and outliving them all, so packing is the
    /// store's business rather than a transaction's (see [`josh_memodb::registry`]). An
    /// ephemeral transaction gets a private store instead, reading through to the shared one.
    mem_odb: std::sync::Arc<josh_memodb::MemOdb>,
    /// The object-database facade: this transaction's store in front of the repository's
    /// objects, plus the runtime alternates registered on it. Built once per transaction so
    /// its cache serves a whole filter run (see [`josh_memodb::Odb`]).
    odb: josh_memodb::Odb,
    /// Ref edits accepted by this transaction but not yet visible on disk. Reads overlay this
    /// queue, and disk boundaries flush objects before applying it.
    pending_refs: std::cell::RefCell<Vec<PendingRef>>,
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

        // Ephemeral transactions discard both their private object store and every ref edit.
        if self.ephemeral {
            return;
        }

        // Objects first, refs second: a ref that reaches disk must never point at an object
        // only memory holds. A flush error is logged and the refs written anyway -- the same
        // end state as before, where refs were already on disk by the time the flush ran.
        let publishes_object = self
            .pending_refs
            .borrow()
            .iter()
            .any(|edit| matches!(&edit.change, PendingRefChange::Update { .. }));
        if publishes_object && let Err(e) = self.mem_odb.flush() {
            log::error!("failed to flush in-memory object store: {e}");
        }
        if let Err(e) = self.apply_pending_refs() {
            log::error!("failed to write pending ref updates: {e}");
        }
    }
}

impl Transaction {
    fn new(
        repo: git2::Repository,
        gix_repo: std::sync::Arc<gix::ThreadSafeRepository>,
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

        // An ephemeral transaction must not contribute to a store that outlives it, so it
        // buffers privately and reads through to the shared one.
        let objects_dir = josh_memodb::objects_dir(&repo);
        let shared = josh_memodb::registry::shared(mem_odb_limit, &objects_dir);
        let mem_odb = if ephemeral {
            josh_memodb::MemOdb::chained(mem_odb_limit, objects_dir, shared)
        } else {
            shared
        };
        // The store the repository was opened with, so pack indices are loaded once for every
        // transaction of this context.
        let odb = josh_memodb::Odb::new(mem_odb.clone(), gix_repo.objects.clone());

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
            trigram_indexer: Default::default(),
            repo,
            gix_repo,
            mem_odb,
            odb,
            pending_refs: std::cell::RefCell::new(Vec::new()),
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

    /// A gitoxide handle on this repository. Made per call rather than held: the handle
    /// itself is not `Send`, and building one is a matter of cloning `Arc`s and taking
    /// snapshot references -- no filesystem access.
    pub fn repo(&self) -> gix::Repository {
        self.gix_repo.to_thread_local()
    }

    /// The libgit2 handle on this repository, for the porcelain josh has not moved to gix:
    /// worktree and index operations, `FETCH_HEAD`'s multi-entry semantics, notes, and
    /// git's DWIM name resolution. It must never read or write objects josh produces --
    /// those live in this transaction's store until it flushes, and this handle cannot see
    /// them (use [`Transaction::odb`]).
    pub fn git2_repo(&self) -> &git2::Repository {
        &self.repo
    }

    /// The transaction's object-database facade: memory store first, repository objects
    /// fallback (see [`josh_memodb::Odb`]).
    pub fn odb(&self) -> &josh_memodb::Odb {
        &self.odb
    }

    /// Add `path` (an objects directory) as a runtime alternate: the facade reads through it
    /// and never buffers what it holds (see [`josh_memodb::Odb::write`]). The porcelain handle
    /// is told separately, since it resolves objects of its own.
    pub fn add_disk_alternate(&self, path: &str) -> anyhow::Result<()> {
        self.repo.odb()?.add_disk_alternate(path)?;
        self.odb.add_alternate(std::path::Path::new(path))?;
        Ok(())
    }

    /// Read the raw bytes of the tree `oid` through the per-transaction [`TreeCache`]
    /// (see there for the promotion and eviction policy). `Ok(None)` means the object exists
    /// but is not a tree; a missing object is an error, like a plain odb read.
    ///
    /// Trees still buffered in the memory store come back as zero-copy `Arc` clones of the
    /// store's own buffers and bypass the TreeCache promotion accounting -- caching them
    /// again would only duplicate memory the store already holds.
    pub fn read_tree_bytes(
        &self,
        odb: &josh_memodb::Odb,
        oid: git2::Oid,
    ) -> anyhow::Result<Option<TreeBytes>> {
        if let Some(bytes) = self.t2.borrow().tree_cache.get(oid) {
            return Ok(Some(TreeBytes::Cached(bytes)));
        }
        let (kind, bytes) = odb.read(oid)?;
        if kind != gix_object::Kind::Tree {
            return Ok(None);
        }
        let obj = match bytes {
            josh_memodb::Bytes::Mem(bytes) => return Ok(Some(TreeBytes::Cached(bytes))),
            josh_memodb::Bytes::Disk(obj) => obj,
        };
        let mut t2 = self.t2.borrow_mut();
        if t2.tree_cache.should_promote(oid) {
            let bytes: std::sync::Arc<[u8]> = obj.into();
            t2.tree_cache.insert(oid, bytes.clone());
            return Ok(Some(TreeBytes::Cached(bytes)));
        }
        Ok(Some(TreeBytes::Odb(obj)))
    }

    /// Pack the repository's buffered objects and block until they are durable, then publish
    /// every ref update a persistent transaction has accepted. Ephemeral transactions keep
    /// their pending refs private. The shared store covers everything buffered for the
    /// repository, not only what this transaction wrote.
    // TODO: remove and rework proxy git launch path to use spawn_git
    pub fn flush_mem_odb(&self) -> anyhow::Result<()> {
        self.mem_odb.flush()?;
        if !self.ephemeral {
            self.apply_pending_refs()?;
        }
        Ok(())
    }

    /// Flush this transaction's in-memory objects and publish its pending ref updates, then build
    /// a `git` subprocess against its repo. Use this in place of [`crate::git::GitCommand::new`]
    /// whenever a transaction is in scope: the spawned `git` reads objects and refs from disk.
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

    fn pending_target(&self, refname: &str) -> Option<Option<PendingTarget>> {
        self.pending_refs
            .borrow()
            .iter()
            .rev()
            .find(|edit| edit.name == refname)
            .map(|edit| match &edit.change {
                PendingRefChange::Update { target, .. } => Some(PendingTarget::Object(*target)),
                PendingRefChange::Delete { .. } => None,
                PendingRefChange::Symbolic { target, .. } => {
                    Some(PendingTarget::Symbolic(target.clone()))
                }
            })
    }

    fn ref_target(&self, refname: &str) -> anyhow::Result<Option<PendingTarget>> {
        if let Some(target) = self.pending_target(refname) {
            return Ok(target);
        }
        match self.repo.find_reference(refname) {
            Ok(reference) => {
                if let Some(target) = reference.symbolic_target()? {
                    Ok(Some(PendingTarget::Symbolic(target.to_owned())))
                } else {
                    Ok(reference.target().map(PendingTarget::Object))
                }
            }
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn resolve_ref_target(&self, refname: &str) -> anyhow::Result<Option<(String, git2::Oid)>> {
        let mut name = refname.to_owned();
        for _ in 0..5 {
            match self.ref_target(&name)? {
                Some(PendingTarget::Object(target)) => return Ok(Some((name, target))),
                Some(PendingTarget::Symbolic(target)) => name = target,
                None => return Ok(None),
            }
        }
        Err(anyhow!("symbolic ref '{}' is nested too deeply", name))
    }

    fn pending_partial(&self, partial: &str) -> Option<Option<String>> {
        let mut candidates = vec![partial.to_owned()];
        if !partial.starts_with("refs/") {
            candidates.extend([
                format!("refs/{partial}"),
                format!("refs/tags/{partial}"),
                format!("refs/heads/{partial}"),
                format!("refs/remotes/{partial}"),
            ]);
            if partial != "HEAD" {
                candidates.push(format!("refs/remotes/{partial}/HEAD"));
            }
        }
        let pending = self.pending_refs.borrow();
        for candidate in candidates {
            if let Some(edit) = pending.iter().rev().find(|edit| edit.name == candidate) {
                return Some(match edit.change {
                    PendingRefChange::Delete { .. } => None,
                    _ => Some(edit.name.clone()),
                });
            }
        }
        None
    }

    fn apply_pending_refs(&self) -> anyhow::Result<()> {
        let edits = std::mem::take(&mut *self.pending_refs.borrow_mut());
        for edit in edits {
            match edit.change {
                PendingRefChange::Update {
                    expected,
                    target,
                    log_message,
                } => self.write_ref(&edit.name, expected, target, &log_message)?,
                PendingRefChange::Delete { expected } => {
                    self.delete_ref_now(&edit.name, expected)?
                }
                PendingRefChange::Symbolic {
                    target,
                    log_message,
                } => {
                    self.repo
                        .reference_symbolic(&edit.name, &target, true, &log_message)?;
                }
            }
        }
        Ok(())
    }

    fn write_ref(
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

    fn delete_ref_now(&self, refname: &str, expected: Expected) -> anyhow::Result<()> {
        match expected {
            Expected::Absent => Err(anyhow!("delete_ref: Expected::Absent is not a valid guard")),
            Expected::Any => match self.repo.find_reference(refname) {
                Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(()),
                Err(e) => Err(e.into()),
                Ok(mut reference) => match reference.delete() {
                    Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(()),
                    other => Ok(other?),
                },
            },
            Expected::At(old) => {
                let mut reference = self.repo.find_reference(refname)?;
                if reference.target() != Some(old) {
                    return Err(anyhow!(
                        "delete_ref: '{}' does not point at {}",
                        refname,
                        old
                    ));
                }
                Ok(reference.delete()?)
            }
        }
    }
    /// Resolve a fully qualified refname through this transaction's pending overlay.
    pub fn resolve_ref(&self, refname: &str) -> anyhow::Result<Option<git2::Oid>> {
        Ok(self.resolve_ref_target(refname)?.map(|(_, target)| target))
    }
    /// The repository's git directory, for the callers that build paths beside it or hand
    /// it to a `git` subprocess.
    pub fn path(&self) -> &std::path::Path {
        self.repo.path()
    }

    /// Where HEAD points. Errors when HEAD is unborn (a repository whose HEAD names a
    /// branch that does not exist yet) or detached at an object that is not a commit;
    /// annotated tags are peeled. The commit is resolved through the transaction's objects,
    /// so a HEAD moved to a commit this transaction produced resolves. (gix mapping:
    /// `Repository::head()` + `head_id()`.)
    pub fn head(&self) -> anyhow::Result<Head> {
        let (name, target) = self
            .resolve_ref_target("HEAD")?
            .ok_or_else(|| anyhow!("HEAD does not point at an object"))?;
        let reference = if name.starts_with("refs/heads/") {
            name
        } else {
            "HEAD".to_string()
        };
        Ok(Head {
            reference,
            target,
            commit: crate::objects::peel_to_commit(self.odb(), target)?,
        })
    }

    /// Resolve a user-supplied revision -- a ref name, a short or full oid, or rev syntax
    /// like `master~2` -- to the object it names, unpeeled. `Ok(None)` when it resolves to
    /// nothing, which covers both a malformed spec and one naming something absent: a
    /// revision a user typed is input, not a contract. (gix mapping: `rev_parse_single`.)
    pub fn rev_parse(&self, spec: &str) -> anyhow::Result<Option<git2::Oid>> {
        match self.repo.revparse_single(spec) {
            Ok(object) => Ok(Some(object.id())),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) if e.code() == git2::ErrorCode::InvalidSpec => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// The fully qualified name of the ref a short name refers to, resolved the way git
    /// resolves an argument that could name several things (`master` ->
    /// `refs/heads/master`). `Ok(None)` when no ref matches. (gix mapping:
    /// `find_reference` with a partial name.)
    pub fn expand_ref_name(&self, short_name: &str) -> anyhow::Result<Option<String>> {
        if let Some(pending) = self.pending_partial(short_name) {
            return match pending {
                Some(name) => Ok(self.resolve_ref_target(&name)?.map(|(name, _)| name)),
                None => Ok(None),
            };
        }
        match self.repo.resolve_reference_from_short_name(short_name) {
            Ok(reference) => Ok(reference
                .resolve()?
                .name()
                .ok()
                .map(|name| name.to_string())),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) if e.code() == git2::ErrorCode::InvalidSpec => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// The remote-tracking ref that `branch_ref` is configured to track, from
    /// `branch.<name>.remote` and `branch.<name>.merge`. `Ok(None)` when the branch has no
    /// upstream configured. (gix mapping: `branch_remote_tracking_ref_name`.)
    pub fn upstream_ref(&self, branch_ref: &str) -> anyhow::Result<Option<String>> {
        match self.repo.branch_upstream_name(branch_ref) {
            Ok(buf) => Ok(buf.as_str().ok().map(|name| name.to_string())),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// A string value from the repository's configuration, `None` when the key is unset.
    /// (gix mapping: `config_snapshot().string()`.)
    pub fn config_string(&self, key: &str) -> anyhow::Result<Option<String>> {
        match self.repo.config()?.get_string(key) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// The identity to record as author and committer, from the repository's configuration
    /// with the usual environment overrides. Errors when no identity is configured.
    /// (gix mapping: `committer()`.)
    pub fn signature(&self) -> anyhow::Result<git2::Signature<'static>> {
        Ok(self.repo.signature()?)
    }

    /// Collect a direct ref update. Transaction reads see it immediately; disk sees it only
    /// after the object store has been flushed at a boundary or on drop.
    pub fn update_ref(
        &self,
        refname: &str,
        expected: Expected,
        target: git2::Oid,
        log_message: &str,
    ) -> anyhow::Result<()> {
        let current = self.ref_target(refname)?;
        let guard_allows_unchanged = match expected {
            Expected::Any => true,
            Expected::At(old) => old == target,
            Expected::Absent => false,
        };
        if guard_allows_unchanged
            && matches!(current, Some(PendingTarget::Object(current)) if current == target)
        {
            return Ok(());
        }
        match expected {
            Expected::Any => {}
            Expected::Absent if current.is_none() => {}
            Expected::At(old) if matches!(current, Some(PendingTarget::Object(current)) if current == old) =>
                {}
            Expected::Absent => {
                return Err(anyhow!(
                    "ref '{}' exists but was expected to be absent",
                    refname
                ));
            }
            Expected::At(old) => {
                return Err(anyhow!(
                    "ref '{}' does not point at the expected value {old}",
                    refname
                ));
            }
        }
        self.pending_refs.borrow_mut().push(PendingRef {
            name: refname.to_owned(),
            change: PendingRefChange::Update {
                expected,
                target,
                log_message: log_message.to_owned(),
            },
        });
        Ok(())
    }
    /// Delete the ref `refname`, guarded by `expected`: `Expected::Any` deletes whatever
    /// is there and treats an absent ref as a no-op; `Expected::At(oid)` asserts the ref
    /// currently points at `oid` and errors, leaving the ref in place, on mismatch or
    /// absence. `Expected::Absent` is a contract error. The ref entry itself is deleted
    /// (a symbolic ref is deleted, not followed); loose and packed entries and the reflog
    /// are removed. Under git2 an `Any` delete of a ref concurrently modified (not
    /// deleted) mid-call may error (libgit2 CASes against the value read at find time);
    /// gix's Any-delete succeeds -- acceptable widening at flag day. (gix mapping: RefEdit
    /// Change::Delete with PreviousValue::Any / MustExistAndMatch, RefLog::AndReference.)
    pub fn delete_ref(&self, refname: &str, expected: Expected) -> anyhow::Result<()> {
        if expected == Expected::Absent {
            return Err(anyhow!("delete_ref: Expected::Absent is not a valid guard"));
        }
        if let Expected::At(old) = expected
            && !matches!(self.ref_target(refname)?, Some(PendingTarget::Object(target)) if target == old)
        {
            return Err(anyhow!(
                "delete_ref: '{}' does not point at {}",
                refname,
                old
            ));
        }
        self.pending_refs.borrow_mut().push(PendingRef {
            name: refname.to_owned(),
            change: PendingRefChange::Delete { expected },
        });
        Ok(())
    }

    /// Force-create or update the symbolic ref `refname` to point at the ref named
    /// `target`, which is validated for refname format but need not exist (dangling
    /// symrefs are allowed). Always overwrites, like `Expected::Any`; grow a guard
    /// parameter only when a consumer needs one (as update_ref did). (gix mapping:
    /// RefEdit Change::Update with Target::Symbolic, PreviousValue::Any.)
    pub fn create_symref(
        &self,
        refname: &str,
        target: &str,
        log_message: &str,
    ) -> anyhow::Result<()> {
        self.pending_refs.borrow_mut().push(PendingRef {
            name: refname.to_owned(),
            change: PendingRefChange::Symbolic {
                target: target.to_owned(),
                log_message: log_message.to_owned(),
            },
        });
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
        let mut refs = HashMap::new();
        for reference in self.repo.references_glob(&format!("{}*", prefix))? {
            let reference = reference?;
            if let (Ok(name), Some(target)) = (reference.name(), reference.target()) {
                refs.insert(name.to_owned(), target);
            }
        }
        for edit in self.pending_refs.borrow().iter() {
            if !edit.name.starts_with(prefix) {
                continue;
            }
            match edit.change {
                PendingRefChange::Update { target, .. } => {
                    refs.insert(edit.name.clone(), target);
                }
                PendingRefChange::Delete { .. } | PendingRefChange::Symbolic { .. } => {
                    refs.remove(&edit.name);
                }
            }
        }
        let mut refs: Vec<_> = refs.into_iter().collect();
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
        if self.odb().contains(oid) {
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
            && self.odb().contains(*oid)
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

        // In addition to commits that are explicitly requested to be stored, also store the
        // sample points, so that every backward walk reaches a stored entry within a bounded
        // number of steps -- including across merge jumps and at orphans. This is what keeps
        // filters that reduce the history length by a very large factor (which store almost
        // nothing of their own) from re-walking all of history on every request.
        if store || hint.is_sample_point() {
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

            if self.odb().contains(oid) {
                // Only report an object as cached if it exists in the object database.
                // This forces a rebuild in case the object was garbage collected.
                return Ok(Some(oid));
            }
        }

        Ok(None)
    }
}

impl Transaction {
    /// The transaction-lifetime josh-search indexer, to pass to `josh_search::trigram_index`
    /// alongside the [`IndexCache`] from [`Transaction::trigram_index_cache`].
    pub fn trigram_indexer(&self) -> std::cell::RefMut<'_, josh_search::Indexer> {
        self.trigram_indexer.borrow_mut()
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
        let repo = transaction.git2_repo();
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
            .git2_repo()
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
            .git2_repo()
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
        transaction.apply_pending_refs().unwrap();
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
        transaction.apply_pending_refs().unwrap();

        transaction
            .git2_repo()
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

    #[test]

    fn delete_ref_any_deletes_existing() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/josh/a", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .delete_ref("refs/josh/a", Expected::Any)
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/a").unwrap(), None);
    }

    #[test]
    fn delete_ref_any_absent_is_noop() {
        let (_dir, transaction) = test_transaction();
        transaction
            .delete_ref("refs/josh/missing", Expected::Any)
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/missing").unwrap(), None);
    }

    #[test]
    fn delete_ref_at_deletes_on_match() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/josh/a", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .delete_ref("refs/josh/a", Expected::At(oid))
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/a").unwrap(), None);
    }

    #[test]
    fn delete_ref_at_fails_on_mismatch() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/josh/a", Expected::Any, a, "test")
            .unwrap();
        assert!(
            transaction
                .delete_ref("refs/josh/a", Expected::At(b))
                .is_err()
        );
        assert_eq!(transaction.resolve_ref("refs/josh/a").unwrap(), Some(a));
    }

    #[test]
    fn delete_ref_at_fails_on_missing_ref() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        assert!(
            transaction
                .delete_ref("refs/josh/missing", Expected::At(a))
                .is_err()
        );
    }

    #[test]
    fn delete_ref_absent_is_contract_error() {
        let (_dir, transaction) = test_transaction();
        assert!(
            transaction
                .delete_ref("refs/josh/missing", Expected::Absent)
                .is_err()
        );
    }

    #[test]
    fn delete_ref_removes_packed_ref() {
        let (dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        transaction
            .update_ref("refs/josh/a", Expected::Any, a, "test")
            .unwrap();
        transaction.apply_pending_refs().unwrap();
        // Pack the ref by hand (git2 exposes no pack-refs API): delete must remove the
        // packed entry, not conclude the ref is absent.
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
            .delete_ref("refs/josh/a", Expected::Any)
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/a").unwrap(), None);
    }

    #[test]
    fn create_symref_resolves_through() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/heads/main", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .create_symref("refs/josh/sym", "refs/heads/main", "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/sym").unwrap(), Some(oid));
    }

    #[test]
    fn create_symref_dangling_target_allowed() {
        let (_dir, transaction) = test_transaction();
        transaction
            .create_symref("refs/josh/sym", "refs/heads/missing", "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/sym").unwrap(), None);
    }

    #[test]
    fn create_symref_overwrites() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/a", Expected::Any, a, "test")
            .unwrap();
        transaction
            .update_ref("refs/heads/b", Expected::Any, b, "test")
            .unwrap();
        transaction
            .create_symref("refs/josh/sym", "refs/heads/a", "test")
            .unwrap();
        transaction
            .create_symref("refs/josh/sym", "refs/heads/b", "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/josh/sym").unwrap(), Some(b));
    }

    #[test]
    fn create_symref_skipped_by_for_each_ref_prefixed() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/josh/a", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .create_symref("refs/josh/sym", "refs/josh/a", "test")
            .unwrap();

        let mut seen = vec![];
        transaction
            .for_each_ref_prefixed("refs/josh/", |name, _| {
                seen.push(name.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, ["refs/josh/a"]);
    }

    #[test]
    fn delete_ref_any_deletes_symref_not_target() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/heads/main", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .create_symref("refs/josh/sym", "refs/heads/main", "test")
            .unwrap();
        transaction
            .delete_ref("refs/josh/sym", Expected::Any)
            .unwrap();
        // The symref entry is gone; the branch it pointed at survives.
        assert!(
            transaction
                .git2_repo()
                .find_reference("refs/josh/sym")
                .is_err()
        );
        assert_eq!(
            transaction.resolve_ref("refs/heads/main").unwrap(),
            Some(oid)
        );
    }

    /// A context the store-lifetime tests below open several transactions from in turn.
    fn test_context() -> (tempfile::TempDir, TransactionContext) {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(dir.path()).unwrap();
        let context = TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(crate::cache::CacheStack::new()),
        );
        (dir, context)
    }

    fn packs(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir.join("objects").join("pack"))
            .map(|entries| {
                entries
                    .filter(|e| {
                        e.as_ref()
                            .is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".pack"))
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// The store outlives the transaction that filled it: a transaction that publishes nothing
    /// leaves no packfile behind, and the objects it produced are still there for the next
    /// transaction to read.
    #[test]
    fn objects_outlive_a_transaction_that_published_nothing() {
        let (dir, context) = test_context();

        let oid = {
            let transaction = context.open().unwrap();
            transaction
                .odb()
                .write(gix_object::Kind::Blob, b"unpublished")
        };

        assert_eq!(packs(dir.path()), 0, "nothing was published, nothing packs");

        let transaction = context.open().unwrap();
        let odb = transaction.odb();
        assert!(odb.contains(oid));
        assert_eq!(&*odb.read(oid).unwrap().1, b"unpublished");
    }

    /// A publishing transaction keeps its ref private until drop, then flushes the object
    /// before making the ref visible to disk-only readers.
    #[test]
    fn publishing_a_ref_flushes_before_ref_reaches_disk() {
        let (dir, context) = test_context();

        let oid = {
            let transaction = context.open().unwrap();
            let oid = transaction
                .odb()
                .write(gix_object::Kind::Blob, b"published");
            transaction
                .update_ref("refs/josh/blob", Expected::Absent, oid, "test")
                .unwrap();

            // Transaction reads see their pending state; disk-only readers see neither half.
            assert_eq!(
                transaction.resolve_ref("refs/josh/blob").unwrap(),
                Some(oid)
            );
            let on_disk = git2::Repository::open(dir.path()).unwrap();
            assert!(on_disk.find_reference("refs/josh/blob").is_err());
            assert!(on_disk.find_blob(oid).is_err());
            oid
        };

        let on_disk = git2::Repository::open(dir.path()).unwrap();
        assert_eq!(
            on_disk.find_reference("refs/josh/blob").unwrap().target(),
            Some(oid)
        );
        assert_eq!(on_disk.find_blob(oid).unwrap().content(), b"published");
    }

    /// An explicit disk-reader boundary has the same ordering as drop: object first, ref second.
    #[test]
    fn flush_mem_odb_publishes_pending_refs_after_objects() {
        let (dir, context) = test_context();
        let transaction = context.open().unwrap();
        let oid = transaction.odb().write(gix_object::Kind::Blob, b"boundary");
        transaction
            .update_ref("refs/josh/blob", Expected::Absent, oid, "test")
            .unwrap();

        transaction.flush_mem_odb().unwrap();

        let on_disk = git2::Repository::open(dir.path()).unwrap();
        assert_eq!(
            on_disk.find_reference("refs/josh/blob").unwrap().target(),
            Some(oid)
        );
        assert_eq!(on_disk.find_blob(oid).unwrap().content(), b"boundary");
    }

    #[test]
    fn ephemeral_transaction_never_publishes_refs() {
        let (dir, _) = test_context();
        {
            let transaction = TransactionContext::new(
                dir.path(),
                std::sync::Arc::new(crate::cache::CacheStack::new()),
            )
            .ephemeral()
            .open()
            .unwrap();
            let target = commit(&transaction, "a");
            transaction
                .update_ref("refs/josh/ephemeral", Expected::Absent, target, "test")
                .unwrap();

            assert_eq!(
                transaction.resolve_ref("refs/josh/ephemeral").unwrap(),
                Some(target)
            );
            transaction.flush_mem_odb().unwrap();
            assert!(
                git2::Repository::open(dir.path())
                    .unwrap()
                    .find_reference("refs/josh/ephemeral")
                    .is_err()
            );
        }

        assert!(
            git2::Repository::open(dir.path())
                .unwrap()
                .find_reference("refs/josh/ephemeral")
                .is_err()
        );
    }

    /// An ephemeral transaction discards what it writes, but still reads what the repository's
    /// store holds in memory.
    #[test]
    fn ephemeral_reads_the_shared_store_and_discards_its_own() {
        let (dir, context) = test_context();

        let shared = {
            let transaction = context.open().unwrap();
            transaction.odb().write(gix_object::Kind::Blob, b"shared")
        };

        let own = {
            let ephemeral = TransactionContext::new(
                dir.path(),
                std::sync::Arc::new(crate::cache::CacheStack::new()),
            )
            .ephemeral()
            .open()
            .unwrap();
            let odb = ephemeral.odb();
            assert_eq!(&*odb.read(shared).unwrap().1, b"shared");
            odb.write(gix_object::Kind::Blob, b"ephemeral")
        };

        // What the ephemeral transaction wrote is gone; what it read is still buffered.
        let transaction = context.open().unwrap();
        let odb = transaction.odb();
        assert!(odb.contains(shared));
        assert!(!odb.contains(own));
        assert_eq!(packs(dir.path()), 0);
    }
}
