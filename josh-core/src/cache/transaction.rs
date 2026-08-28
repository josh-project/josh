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
        commit_oid: gix_hash::ObjectId,
        arg: &str,
    ) -> anyhow::Result<crate::filter::Filter>;
}

/// Where a repository's HEAD points, as [`Transaction::head`] reports it.
pub struct Head {
    /// The ref HEAD resolves to: a fully qualified `refs/heads/...` on a branch, and `HEAD`
    /// itself when detached. Either way, this is the ref to update to move HEAD.
    pub reference: String,
    /// The unpeeled target of [`Head::reference`], for guarding an update against it.
    pub target: gix_hash::ObjectId,
    /// The commit HEAD resolves to, annotated tags peeled.
    pub commit: gix_hash::ObjectId,
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
    At(gix_hash::ObjectId),
}

/// Parse `refname` as the fully qualified name every ref API method requires it to be.
fn full_ref_name(refname: &str) -> anyhow::Result<gix::refs::FullName> {
    gix::refs::FullName::try_from(refname)
        .map_err(|e| anyhow!("'{}' is not a valid refname: {}", refname, e))
}

/// Who a reflog entry names. A missing configured identity must not prevent ref updates from
/// writing their reflogs.
fn reflog_committer(signature: Option<&gix_actor::Signature>) -> gix_actor::Signature {
    signature.cloned().unwrap_or_else(|| gix_actor::Signature {
        name: "unknown".into(),
        email: "unknown".into(),
        time: gix_actor::date::Time::now_local_or_utc(),
    })
}

/// The gix guard an [`Expected`] is.
fn previous_value(expected: Expected) -> gix::refs::transaction::PreviousValue {
    use gix::refs::transaction::PreviousValue;
    match expected {
        Expected::Any => PreviousValue::Any,
        Expected::Absent => PreviousValue::MustNotExist,
        Expected::At(old) => PreviousValue::MustExistAndMatch(gix::refs::Target::Object(old)),
    }
}

static REF_CACHE: LazyLock<
    RwLock<HashMap<gix_hash::ObjectId, HashMap<gix_hash::ObjectId, gix_hash::ObjectId>>>,
> = LazyLock::new(Default::default);

static POPULATE_MAP: LazyLock<
    RwLock<HashMap<(gix_hash::ObjectId, gix_hash::ObjectId), gix_hash::ObjectId>>,
> = LazyLock::new(Default::default);

// Keyed by (input tree, pattern key, NFA state mask). The state mask makes entries independent
// of the path a subtree was reached through; the legacy full-path fallback folds its root path
// into a synthetic pattern key and uses mask 0.
static GLOB_MAP: LazyLock<
    RwLock<HashMap<(gix_hash::ObjectId, gix_hash::ObjectId, u64), gix_hash::ObjectId>>,
> = LazyLock::new(Default::default);

// Path-projection memoization for `:PATHS` and its inverse, keyed by (input tree oid, root path).
// Both are pure functions of the input tree, and workspace filters walk commits parent-first, so a
// child commit reuses the projections its parent just computed for the subtrees they share, which
// an in-process map captures.
static PATHS_MAP: LazyLock<RwLock<HashMap<(gix_hash::ObjectId, String), gix_hash::ObjectId>>> =
    LazyLock::new(Default::default);

static INVERT_MAP: LazyLock<RwLock<HashMap<(gix_hash::ObjectId, String), gix_hash::ObjectId>>> =
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

    // Object-database environment overrides must not affect a transaction opened for an
    // explicit path. System, user and repository configuration plus identity environment
    // variables remain available to configuration and identity queries.
    let mut options = gix::open::Options::isolated();
    options.permissions.config = gix::open::permissions::Config::all();
    options.permissions.config.env = false;
    options.permissions.env.xdg_config_home = gix::sec::Permission::Allow;
    options.permissions.env.home = gix::sec::Permission::Allow;
    options.permissions.env.identity = gix::sec::Permission::Allow;
    let repo = std::sync::Arc::new(gix::ThreadSafeRepository::open_opts(path, options)?);
    repos.insert(path.to_owned(), repo.clone());

    Ok(repo)
}

impl TransactionContext {
    pub fn from_env(cache: std::sync::Arc<CacheStack>) -> anyhow::Result<Self> {
        let repo = gix::ThreadSafeRepository::discover_with_environment_overrides(
            std::env::current_dir()?,
        )
        .map_err(crate::git::map_discovery_error)?;
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
    commit_map: HashMap<gix_hash::ObjectId, HashMap<gix_hash::ObjectId, gix_hash::ObjectId>>,
    apply_map: HashMap<gix_hash::ObjectId, HashMap<gix_hash::ObjectId, gix_hash::ObjectId>>,
    subtract_map: HashMap<(gix_hash::ObjectId, gix_hash::ObjectId), gix_hash::ObjectId>,
    intersect_map: HashMap<(gix_hash::ObjectId, gix_hash::ObjectId), gix_hash::ObjectId>,
    overlay_map: HashMap<(gix_hash::ObjectId, gix_hash::ObjectId), gix_hash::ObjectId>,
    unapply_map: HashMap<gix_hash::ObjectId, HashMap<gix_hash::ObjectId, gix_hash::ObjectId>>,
    legalize_map: HashMap<(crate::filter::Filter, gix_hash::ObjectId), crate::filter::Filter>,
    downstack_deps_map:
        HashMap<gix_hash::ObjectId, std::collections::HashSet<crate::filter::DownstackDep>>,
    merge_trees_map:
        HashMap<(gix_hash::ObjectId, gix_hash::ObjectId, gix_hash::ObjectId), gix_hash::ObjectId>,
    last_written_commit: Option<(gix_hash::ObjectId, gix_hash::ObjectId)>,
    tree_cache: TreeCache,

    cache: std::sync::Arc<CacheStack>,
    // In-transaction memoization of the trigram index (source tree -> index tree); the
    // cache backend behind it holds the durable, cross-transaction copy.
    index_map: HashMap<gix_hash::ObjectId, gix_hash::ObjectId>,
    missing: Vec<(usize, crate::filter::Filter, gix_hash::ObjectId)>,
    misses: usize,
    nesting_level: usize,
}

pub struct Transaction {
    t2: std::cell::RefCell<Transaction2>,
    /// josh-search indexing state kept for the whole transaction, so indexing a chain of
    /// commits reuses the merge work of earlier commits. Its own cell because `trigram_index`
    /// borrows it for the entire call while also borrowing other caches through `t2`.
    trigram_indexer: std::cell::RefCell<josh_search::Indexer>,
    /// The primary repository view for object and ref reads. Held in its thread-safe form:
    /// a transaction crosses threads (josh-graphql requires `Send`) while a `gix::Repository`
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
    /// Ref updates this transaction has accepted but not yet written to disk. They are
    /// applied after the store has been flushed -- at an explicit boundary
    /// ([`Transaction::flush_mem_odb`], [`Transaction::git_command`]) or, failing that, on
    /// drop -- so a ref only ever becomes visible once the objects it points at are durable.
    /// Reads fold the pending state in (see [`Transaction::find_ref`]), so a caller cannot
    /// tell the difference.
    pending_refs: std::cell::RefCell<Vec<gix::refs::transaction::RefEdit>>,
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
        // only memory holds. Repository maintenance preserves unreachable objects during the
        // publication gap; see `josh_memodb::pack::write_snapshot`.
        let publishes_object = self.pending_refs.borrow().iter().any(|edit| {
            matches!(
                &edit.change,
                gix::refs::transaction::Change::Update {
                    new: gix::refs::Target::Object(_),
                    ..
                }
            )
        });
        let objects_are_durable = if publishes_object {
            match self.mem_odb.flush() {
                Ok(()) => true,
                Err(error) => {
                    log::error!("failed to flush in-memory object store: {error}");
                    false
                }
            }
        } else {
            true
        };
        if objects_are_durable && let Err(error) = self.apply_pending_refs() {
            log::error!("failed to write pending ref updates: {error}");
        }
    }
}

impl Transaction {
    fn new(
        gix_repo: std::sync::Arc<gix::ThreadSafeRepository>,
        cache: std::sync::Arc<CacheStack>,
        ref_prefix: Option<&str>,
        mem_odb_limit: Option<usize>,
        ephemeral: bool,
    ) -> Transaction {
        // An ephemeral transaction must not contribute to a store that outlives it, so it
        // buffers privately and reads through to the shared one.
        let objects_dir = gix_repo.objects.path().to_owned();
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
            path: self.gix_repo.path().to_owned(),
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

    /// The transaction's object-database facade: memory store first, repository objects
    /// fallback (see [`josh_memodb::Odb`]).
    pub fn odb(&self) -> &josh_memodb::Odb {
        &self.odb
    }

    /// Add `path` (an objects directory) as a runtime alternate: the facade reads through it
    /// and never buffers what it holds (see [`josh_memodb::Odb::write`]).
    pub fn add_disk_alternate(&self, path: &str) -> anyhow::Result<()> {
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
        oid: gix_hash::ObjectId,
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
            self.gix_repo.path(),
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

    /// The gitoxide ref store. Ref work goes through it rather than a [`Transaction::repo`]
    /// handle, which clones an object database a ref read has no use for. Its
    /// packed-refs snapshot is shared with every transaction of the context and mtime-checked,
    /// so refs another process packs are seen.
    fn refs(&self) -> &gix::refs::file::Store {
        &self.gix_repo.refs
    }

    /// The state `name` has once this transaction's collected-but-unwritten edits are folded
    /// in: `None` when no edit touches it, and otherwise the ref's target with a delete
    /// collapsed to `None`. The *last* edit wins, matching the order the writes would have
    /// landed in.
    fn pending_target(&self, name: &gix::refs::FullName) -> Option<Option<gix::refs::Target>> {
        self.pending_refs
            .borrow()
            .iter()
            .rev()
            .find(|edit| &edit.name == name)
            .map(|edit| match &edit.change {
                gix::refs::transaction::Change::Update { new, .. } => Some(new.clone()),
                gix::refs::transaction::Change::Delete { .. } => None,
            })
    }

    /// Look up one fully qualified ref through the pending overlay first and the store
    /// second: the view every ref read of this transaction answers from, so reads observe
    /// writes that have not reached disk yet.
    fn find_ref(&self, name: &gix::refs::FullName) -> anyhow::Result<Option<gix::refs::Reference>> {
        if let Some(target) = self.pending_target(name) {
            return Ok(target.map(|target| gix::refs::Reference {
                name: name.clone(),
                target,
                peeled: None,
            }));
        }
        Ok(self.refs().try_find(name.as_ref())?)
    }

    /// Match this transaction's pending ref names against the expansions git tries for a
    /// partial name (`main` -> `refs/main`, `refs/tags/main`, `refs/heads/main`, ...),
    /// mirroring gix's own candidate order. Pending names enter through `&str` APIs, so
    /// byte-invalid names cannot occur here.
    fn find_pending_partial(&self, partial: &str) -> Option<Option<gix::refs::Reference>> {
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
            if let Some(edit) = pending
                .iter()
                .rev()
                .find(|edit| edit.name.as_bstr() == candidate.as_bytes())
            {
                return Some(match &edit.change {
                    gix::refs::transaction::Change::Update { new, .. } => {
                        Some(gix::refs::Reference {
                            name: edit.name.clone(),
                            target: new.clone(),
                            peeled: None,
                        })
                    }
                    gix::refs::transaction::Change::Delete { .. } => None,
                });
            }
        }
        None
    }

    /// Resolve a fully qualified refname to its target oid, following symbolic refs. No
    /// partial-name DWIM. `Ok(None)` if the ref (or the end of a symbolic chain) does not
    /// exist. The target is not peeled: for an annotated tag ref this is the tag oid,
    /// peeling is an object-store concern.
    pub fn resolve_ref(&self, refname: &str) -> anyhow::Result<Option<gix_hash::ObjectId>> {
        // Parsing as a full name is what rejects `master`: gix's find would resolve it.
        let name = full_ref_name(refname)?;
        let Some(reference) = self.find_ref(&name)? else {
            return Ok(None);
        };
        Ok(self.follow_symrefs(reference)?.map(|(_, target)| target))
    }

    /// The name of the ref a symbolic ref points at, unfollowed. `Ok(None)` when `refname`
    /// does not exist or is a direct ref: the callers reading a remote's advertised HEAD
    /// treat both as "the remote named no default branch".
    pub fn symref_target(&self, refname: &str) -> anyhow::Result<Option<String>> {
        let name = full_ref_name(refname)?;
        let Some(reference) = self.find_ref(&name)? else {
            return Ok(None);
        };
        match &reference.target {
            gix::refs::Target::Object(_) => Ok(None),
            gix::refs::Target::Symbolic(target) => Ok(Some(
                std::str::from_utf8(target.as_bstr())
                    .map_err(|e| {
                        anyhow!("target of symbolic ref '{}' is not UTF-8: {}", refname, e)
                    })?
                    .to_string(),
            )),
        }
    }

    /// Follow `reference` to the direct ref its symbolic chain ends at, and return that ref's
    /// name and unpeeled target. `Ok(None)` for a dangling chain: a ref that points nowhere
    /// reads as missing.
    fn follow_symrefs(
        &self,
        mut reference: gix::refs::Reference,
    ) -> anyhow::Result<Option<(gix::refs::FullName, gix_hash::ObjectId)>> {
        // git's own limit on how far a symbolic ref may point.
        for _ in 0..5 {
            let next = match &reference.target {
                gix::refs::Target::Object(id) => {
                    let target = id.to_owned();
                    return Ok(Some((reference.name, target)));
                }
                gix::refs::Target::Symbolic(next) => self.find_ref(next)?,
            };
            match next {
                Some(next) => reference = next,
                None => return Ok(None),
            }
        }
        Err(anyhow!(
            "symbolic ref '{}' is nested too deeply",
            reference.name.as_bstr()
        ))
    }

    /// The repository's git directory, for the callers that build paths beside it or hand
    /// it to a `git` subprocess.
    pub fn path(&self) -> &std::path::Path {
        self.gix_repo.path()
    }

    /// Where HEAD points. Errors when HEAD is unborn (a repository whose HEAD names a
    /// branch that does not exist yet) or detached at an object that is not a commit;
    /// annotated tags are peeled. The commit is resolved through the transaction's objects,
    /// so a HEAD moved to a commit this transaction produced resolves.
    pub fn head(&self) -> anyhow::Result<Head> {
        let head = self
            .find_ref(&full_ref_name("HEAD")?)?
            .ok_or_else(|| anyhow!("repository has no HEAD"))?;
        let (name, target) = self
            .follow_symrefs(head)?
            .ok_or_else(|| anyhow!("HEAD does not point at an object (unborn or dangling)"))?;
        let name = std::str::from_utf8(name.as_bstr())
            .map_err(|e| anyhow!("HEAD ref name is not valid UTF-8: {}", e))?
            .to_string();
        // Only a branch is the ref to update; a detached HEAD -- and the odd symbolic HEAD
        // pointing outside `refs/heads/` -- is updated as `HEAD` itself.
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
    /// revision a user typed is input, not a contract.
    pub fn rev_parse(&self, spec: &str) -> anyhow::Result<Option<gix_hash::ObjectId>> {
        let repo = self.repo();
        match repo.rev_parse_single(spec) {
            Ok(id) => Ok(Some(id.into())),
            Err(gix::revision::spec::parse::single::Error::RangedRev { .. }) => Ok(None),
            Err(gix::revision::spec::parse::single::Error::Parse(error)) => {
                let operational_error = error.sources().any(|source| {
                    matches!(
                        source.downcast_ref::<gix_object::find::existing::Error>(),
                        Some(gix_object::find::existing::Error::Find(_))
                    ) || source.is::<gix_object::decode::Error>()
                        || source.is::<std::io::Error>()
                });
                if operational_error {
                    Err(error.into())
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// The fully qualified name of the ref a short name refers to, resolved the way git
    /// resolves an argument that could name several things (`master` ->
    /// `refs/heads/master`). Symbolic refs are followed, so `HEAD` expands to the branch it
    /// is on rather than to itself -- this is the name callers write back to. `Ok(None)`
    /// when no ref matches.
    pub fn expand_ref_name(&self, short_name: &str) -> anyhow::Result<Option<String>> {
        // A name that is not even a valid partial refname names no ref, like one that
        // matches nothing: what a user typed is input, not a contract.
        let Ok(partial) = gix::refs::PartialName::try_from(short_name) else {
            return Ok(None);
        };
        let Some(reference) = (match self.find_pending_partial(short_name) {
            Some(reference) => reference,
            None => self.refs().try_find(partial.as_ref())?,
        }) else {
            return Ok(None);
        };
        Ok(self.follow_symrefs(reference)?.and_then(|(name, _)| {
            std::str::from_utf8(name.as_bstr())
                .ok()
                .map(ToOwned::to_owned)
        }))
    }

    /// The remote-tracking ref that `branch_ref` is configured to track, from
    /// `branch.<name>.remote` and `branch.<name>.merge`. `Ok(None)` when the branch has no
    /// upstream configured.
    pub fn upstream_ref(&self, branch_ref: &str) -> anyhow::Result<Option<String>> {
        let branch = gix::refs::FullName::try_from(branch_ref)?;
        let repo = self.repo();
        let Some(upstream) =
            repo.branch_remote_tracking_ref_name(branch.as_ref(), gix::remote::Direction::Fetch)
        else {
            return Ok(None);
        };
        let upstream = upstream?;
        Ok(std::str::from_utf8(upstream.as_bstr())
            .ok()
            .map(ToOwned::to_owned))
    }

    /// A string value from the repository's configuration, `None` when the key is unset.
    pub fn config_string(&self, key: &str) -> anyhow::Result<Option<String>> {
        self.repo()
            .config_snapshot()
            .string(key)
            .map(|value| {
                std::str::from_utf8(value.as_ref())
                    .map(ToOwned::to_owned)
                    .map_err(Into::into)
            })
            .transpose()
    }

    /// The identity to record as author and committer, from the repository's configuration
    /// with the usual environment overrides. Errors when no identity is configured.
    pub fn signature(&self) -> anyhow::Result<gix_actor::Signature> {
        let repo = self.repo();
        let signature = repo
            .committer()
            .ok_or_else(|| anyhow!("no committer identity is configured"))??;
        Ok(signature.to_owned()?)
    }

    /// Create or update the direct ref `refname` to point at `target`, guarded by
    /// `expected`: with `Expected::Any` an existing ref is overwritten unconditionally
    /// (updating to the value a ref already has is a no-op); `Expected::At(oid)` asserts
    /// the ref currently points at `oid`; `Expected::Absent` asserts it does not exist.
    /// Writers that derive `target` from the ref's old value pass `At`/`Absent` so they
    /// never overwrite a concurrent update.
    ///
    /// The write itself is deferred: the edit is collected and applied once this
    /// transaction's objects are durable (see [`Transaction::pending_refs`]), so a ref can
    /// never appear on disk while its target is memory-only. Guard checks happen when the
    /// edit lands, avoiding an eager ref read and closing the race between checking and
    /// writing. A failed guard therefore fails the explicit boundary or is logged by drop.
    pub fn update_ref(
        &self,
        refname: &str,
        expected: Expected,
        target: gix_hash::ObjectId,
        log_message: &str,
    ) -> anyhow::Result<()> {
        let name = full_ref_name(refname)?;
        // A write is a lock file, a write and a rename; a read is a stat. So a ref already
        // pointing at `target` is left alone -- the no-op this contract promises, and most of
        // what a proxy serving a repository whose refs rarely move does. Probing cannot pay
        // where the guard already rules the current value out.
        let guard_allows_unchanged = match expected {
            Expected::Any => true,
            Expected::At(old) => old == target,
            Expected::Absent => false,
        };
        if guard_allows_unchanged
            && let Some(reference) = self.find_ref(&name)?
            && reference.target == gix::refs::Target::Object(target)
        {
            return Ok(());
        }
        self.pending_refs
            .borrow_mut()
            .push(gix::refs::transaction::RefEdit {
                change: gix::refs::transaction::Change::Update {
                    log: gix::refs::transaction::LogChange {
                        mode: gix::refs::transaction::RefLog::AndReference,
                        force_create_reflog: false,
                        message: log_message.into(),
                    },
                    expected: previous_value(expected),
                    new: gix::refs::Target::Object(target),
                },
                name,
                deref: false,
            });
        Ok(())
    }

    /// Run `edits` as one ref transaction, supplying what a `gix::Repository` would have:
    /// lock timeouts and a reflog identity (see [`reflog_committer`]).
    fn edit_refs(
        &self,
        edits: Vec<gix::refs::transaction::RefEdit>,
    ) -> anyhow::Result<Vec<gix::refs::transaction::RefEdit>> {
        use gix::lock::acquire::Fail;
        let committer = reflog_committer(self.signature().ok().as_ref());
        let mut time_buf = gix_actor::date::parse::TimeBuf::default();
        Ok(self
            .refs()
            .transaction()
            // gix's defaults for `core.filesRefLockTimeout` and `core.packedRefsTimeout`.
            .prepare(
                edits,
                Fail::AfterDurationWithBackoff(std::time::Duration::from_millis(100)),
                Fail::AfterDurationWithBackoff(std::time::Duration::from_millis(1000)),
            )?
            .commit(committer.to_ref(&mut time_buf))?)
    }

    /// Apply every collected ref edit in order. gix rejects one transaction that names the
    /// same ref twice, so split the queue into maximal runs with distinct names; that keeps
    /// multi-ref batches cheap while preserving same-ref chains and reflog entries exactly
    /// as if each public API call had written immediately.
    fn apply_pending_refs(&self) -> anyhow::Result<()> {
        let edits = {
            let mut pending = self.pending_refs.borrow_mut();
            if pending.is_empty() {
                return Ok(());
            }
            std::mem::take(&mut *pending)
        };

        let mut first_error = None;
        let mut run = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for edit in edits {
            let name = edit.name.as_bstr().to_owned();
            if seen.contains(&name) {
                self.apply_ref_run(std::mem::take(&mut run), &mut first_error);
                seen.clear();
            }
            seen.insert(name);
            run.push(edit);
        }
        self.apply_ref_run(run, &mut first_error);

        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    fn apply_ref_run(
        &self,
        edits: Vec<gix::refs::transaction::RefEdit>,
        first_error: &mut Option<anyhow::Error>,
    ) {
        if edits.is_empty() {
            return;
        }
        if let Err(error) = self.edit_refs(edits) {
            if first_error.is_some() {
                log::error!("failed to write pending ref updates: {error}");
            } else {
                *first_error = Some(error);
            }
        }
    }

    /// Delete the ref `refname`, guarded by `expected`: `Expected::Any` deletes whatever
    /// is there and treats an absent ref as a no-op; `Expected::At(oid)` asserts the ref
    /// currently points at `oid` and errors, leaving the ref in place, on mismatch or
    /// absence. `Expected::Absent` is a contract error. The ref entry itself is deleted
    /// (a symbolic ref is deleted, not followed); loose and packed entries and the reflog
    /// are removed. `Expected::Any` deliberately carries no concurrency guard.
    pub fn delete_ref(&self, refname: &str, expected: Expected) -> anyhow::Result<()> {
        if expected == Expected::Absent {
            return Err(anyhow!("delete_ref: Expected::Absent is not a valid guard"));
        }
        let name = full_ref_name(refname)?;
        if let Expected::At(old) = expected {
            match self.find_ref(&name)? {
                Some(reference) if reference.target == gix::refs::Target::Object(old) => {}
                _ => {
                    return Err(anyhow!(
                        "ref '{}' does not point at the expected value {old}",
                        refname
                    ));
                }
            }
        }
        self.pending_refs
            .borrow_mut()
            .push(gix::refs::transaction::RefEdit {
                change: gix::refs::transaction::Change::Delete {
                    expected: previous_value(expected),
                    log: gix::refs::transaction::RefLog::AndReference,
                },
                name,
                deref: false,
            });
        Ok(())
    }

    /// Force-create or update the symbolic ref `refname` to point at the ref named
    /// `target`, which is validated for refname format but need not exist (dangling
    /// symrefs are allowed). Always overwrites, like `Expected::Any`; grow a guard
    /// parameter only when a consumer needs one (as update_ref did). No reflog entry is
    /// written because a symbolic update has no oid to record.
    pub fn create_symref(
        &self,
        refname: &str,
        target: &str,
        log_message: &str,
    ) -> anyhow::Result<()> {
        self.pending_refs
            .borrow_mut()
            .push(gix::refs::transaction::RefEdit {
                change: gix::refs::transaction::Change::Update {
                    log: gix::refs::transaction::LogChange {
                        mode: gix::refs::transaction::RefLog::AndReference,
                        force_create_reflog: false,
                        message: log_message.into(),
                    },
                    expected: gix::refs::transaction::PreviousValue::Any,
                    new: gix::refs::Target::Symbolic(full_ref_name(target)?),
                },
                name: full_ref_name(refname)?,
                deref: false,
            });
        Ok(())
    }

    /// Run `cb` for every direct ref whose full name starts with `prefix`, byte-sorted by
    /// name. `prefix` must consist of refname-valid characters. Symbolic refs and refs
    /// with non-UTF-8 names are skipped. Errors from `cb` abort the iteration.
    pub fn for_each_ref_prefixed(
        &self,
        prefix: &str,
        mut cb: impl FnMut(&str, gix_hash::ObjectId) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        // Glob metacharacters (*?[\) are all invalid in refnames, so a caller passing one
        // means to match, not to prefix.
        debug_assert!(
            !prefix.contains(['*', '?', '[', '\\']),
            "prefix must consist of refname-valid characters"
        );
        let store = self.refs();
        let platform = store.iter()?;
        // The directory walk is what keeps this cheap, but gix does it only for a prefix that
        // is also a safe relative path: without a trailing `/` it starts one directory up (so
        // `` would walk above `.git` and `refs` all of it, objects included), and it rejects
        // components a checkout could not create, which josh's percent-encoded upstream
        // namespaces can be. The rest walk every ref, which the name filter makes equivalent.
        let walkable: Option<&gix::path::RelativePath> = prefix
            .contains('/')
            .then(|| prefix.try_into().ok())
            .flatten();
        let iter = match walkable {
            Some(prefix) => platform.prefixed(prefix)?,
            None => platform.all()?,
        };
        let mut refs = HashMap::new();
        for reference in iter {
            let reference = reference?;
            let (Ok(name), gix::refs::Target::Object(target)) = (
                std::str::from_utf8(reference.name.as_bstr()),
                &reference.target,
            ) else {
                continue;
            };
            if !name.starts_with(prefix) {
                continue;
            }
            refs.insert(name.to_owned(), target.to_owned());
        }
        for edit in self.pending_refs.borrow().iter() {
            let Ok(name) = std::str::from_utf8(edit.name.as_bstr()) else {
                continue;
            };
            if !name.starts_with(prefix) {
                continue;
            }
            match &edit.change {
                gix::refs::transaction::Change::Update {
                    new: gix::refs::Target::Object(target),
                    ..
                } => {
                    refs.insert(name.to_owned(), target.to_owned());
                }
                gix::refs::transaction::Change::Update {
                    new: gix::refs::Target::Symbolic(_),
                    ..
                }
                | gix::refs::transaction::Change::Delete { .. } => {
                    refs.remove(name);
                }
            }
        }
        // Byte order of the full names is the contract, not whatever order the store
        // happens to iterate in.
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

    pub fn insert_apply(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
        to: gix_hash::ObjectId,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.apply_map
            .entry(filter.id())
            .or_default()
            .insert(from, to);
    }

    pub fn get_apply(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
    ) -> Option<gix_hash::ObjectId> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.apply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        None
    }

    pub(crate) fn insert_downstack_deps(
        &self,
        oid: gix_hash::ObjectId,
        deps: std::collections::HashSet<crate::filter::DownstackDep>,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.downstack_deps_map.insert(oid, deps);
    }

    pub(crate) fn get_downstack_deps(
        &self,
        oid: gix_hash::ObjectId,
    ) -> Option<std::collections::HashSet<crate::filter::DownstackDep>> {
        let t2 = self.t2.borrow_mut();
        t2.downstack_deps_map.get(&oid).cloned()
    }

    pub(crate) fn insert_merge_trees(
        &self,
        key: (gix_hash::ObjectId, gix_hash::ObjectId, gix_hash::ObjectId),
        result: gix_hash::ObjectId,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.merge_trees_map.insert(key, result);
    }

    pub(crate) fn get_merge_trees(
        &self,
        key: (gix_hash::ObjectId, gix_hash::ObjectId, gix_hash::ObjectId),
    ) -> Option<gix_hash::ObjectId> {
        let t2 = self.t2.borrow_mut();
        t2.merge_trees_map.get(&key).copied()
    }

    pub fn insert_subtract(
        &self,
        from: (gix_hash::ObjectId, gix_hash::ObjectId),
        to: gix_hash::ObjectId,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.subtract_map.insert(from, to);
    }

    pub fn get_subtract(
        &self,
        from: (gix_hash::ObjectId, gix_hash::ObjectId),
    ) -> Option<gix_hash::ObjectId> {
        let t2 = self.t2.borrow_mut();
        t2.subtract_map.get(&from).cloned()
    }

    pub fn insert_intersect(
        &self,
        from: (gix_hash::ObjectId, gix_hash::ObjectId),
        to: gix_hash::ObjectId,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.intersect_map.insert(from, to);
    }

    pub fn get_intersect(
        &self,
        from: (gix_hash::ObjectId, gix_hash::ObjectId),
    ) -> Option<gix_hash::ObjectId> {
        let t2 = self.t2.borrow_mut();
        t2.intersect_map.get(&from).cloned()
    }

    pub fn insert_overlay(
        &self,
        from: (gix_hash::ObjectId, gix_hash::ObjectId),
        to: gix_hash::ObjectId,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.overlay_map.insert(from, to);
    }

    pub fn get_overlay(
        &self,
        from: (gix_hash::ObjectId, gix_hash::ObjectId),
    ) -> Option<gix_hash::ObjectId> {
        let t2 = self.t2.borrow_mut();
        t2.overlay_map.get(&from).cloned()
    }

    /// Remember the most recent commit josh wrote in this transaction as `(oid, tree_id)`. A history
    /// walk processes a commit right after writing its parent, so this single slot answers the
    /// common "what tree does my filtered parent have" lookup without re-parsing the parent from the
    /// odb -- and without retaining every written commit the way a map would.
    pub fn set_last_written_commit(&self, commit: gix_hash::ObjectId, tree: gix_hash::ObjectId) {
        self.t2.borrow_mut().last_written_commit = Some((commit, tree));
    }

    pub fn last_written_commit(&self) -> Option<(gix_hash::ObjectId, gix_hash::ObjectId)> {
        self.t2.borrow().last_written_commit
    }

    pub fn insert_legalize(
        &self,
        from: (crate::filter::Filter, gix_hash::ObjectId),
        to: crate::filter::Filter,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.legalize_map.insert(from, to);
    }

    pub fn get_legalize(
        &self,
        from: (crate::filter::Filter, gix_hash::ObjectId),
    ) -> Option<crate::filter::Filter> {
        let t2 = self.t2.borrow_mut();
        t2.legalize_map.get(&from).cloned()
    }

    pub fn insert_unapply(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
        to: gix_hash::ObjectId,
    ) {
        let mut t2 = self.t2.borrow_mut();
        t2.unapply_map
            .entry(filter.id())
            .or_default()
            .insert(from, to);
    }

    pub fn insert_paths(&self, tree: (gix_hash::ObjectId, String), result: gix_hash::ObjectId) {
        PATHS_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_paths(&self, tree: (gix_hash::ObjectId, String)) -> Option<gix_hash::ObjectId> {
        PATHS_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_invert(&self, tree: (gix_hash::ObjectId, String), result: gix_hash::ObjectId) {
        INVERT_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_invert(&self, tree: (gix_hash::ObjectId, String)) -> Option<gix_hash::ObjectId> {
        INVERT_MAP.read().unwrap().get(&tree).cloned()
    }

    /// Cache indexes under the commit's history shard. A null ID means no commit context.
    pub fn trigram_index_cache(&self, commit: gix_hash::ObjectId) -> TrigramIndexCache<'_> {
        TrigramIndexCache {
            transaction: self,
            hint: self.tree_keyed_hint(commit),
        }
    }

    /// Trees carry no history position of their own, so tree-keyed records use the hint
    /// of the commit being indexed. Falls back to the placeholder when there is no commit
    /// context or the hint cannot be computed.
    fn tree_keyed_hint(&self, commit: gix_hash::ObjectId) -> HistoryGraphHint {
        if commit == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
            return TREE_KEYED_FALLBACK_HINT;
        }
        compute_history_hint(self, commit).unwrap_or(TREE_KEYED_FALLBACK_HINT)
    }

    fn insert_trigram_index(
        &self,
        tree: gix_hash::ObjectId,
        result: gix_hash::ObjectId,
        hint: HistoryGraphHint,
    ) {
        let filter = crate::filter::index();
        let mut t2 = self.t2.borrow_mut();
        t2.index_map.entry(tree).or_insert(result);
        if let Err(e) = t2.cache.write_all(filter, tree, result, hint, true) {
            log::warn!("trigram index cache write failed: {e}");
        }
    }

    fn get_trigram_index(
        &self,
        tree: gix_hash::ObjectId,
        hint: HistoryGraphHint,
    ) -> Option<gix_hash::ObjectId> {
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

    pub fn insert_populate(
        &self,
        tree: (gix_hash::ObjectId, gix_hash::ObjectId),
        result: gix_hash::ObjectId,
    ) {
        POPULATE_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_populate(
        &self,
        tree: (gix_hash::ObjectId, gix_hash::ObjectId),
    ) -> Option<gix_hash::ObjectId> {
        POPULATE_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_glob(
        &self,
        tree: (gix_hash::ObjectId, gix_hash::ObjectId, u64),
        result: gix_hash::ObjectId,
    ) {
        GLOB_MAP.write().unwrap().entry(tree).or_insert(result);
    }

    pub fn get_glob(
        &self,
        tree: (gix_hash::ObjectId, gix_hash::ObjectId, u64),
    ) -> Option<gix_hash::ObjectId> {
        GLOB_MAP.read().unwrap().get(&tree).cloned()
    }

    pub fn insert_ref(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
        to: gix_hash::ObjectId,
    ) {
        REF_CACHE
            .write()
            .unwrap()
            .entry(filter.id())
            .or_default()
            .insert(from, to);
    }

    pub fn get_ref(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
    ) -> Option<gix_hash::ObjectId> {
        if let Some(m) = REF_CACHE.read().unwrap().get(&filter.id())
            && let Some(oid) = m.get(&from)
            && self.odb().contains(*oid)
        {
            return Some(*oid);
        }
        None
    }

    pub fn get_unapply(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
    ) -> Option<gix_hash::ObjectId> {
        let t2 = self.t2.borrow_mut();
        if let Some(m) = t2.unapply_map.get(&filter.id()) {
            return m.get(&from).cloned();
        }
        None
    }

    pub fn lookup_filter_hook(
        &self,
        hook: &str,
        from: gix_hash::ObjectId,
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
        from: gix_hash::ObjectId,
        to: gix_hash::ObjectId,
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

    pub fn get_missing(
        &self,
    ) -> anyhow::Result<Vec<(usize, crate::filter::Filter, gix_hash::ObjectId)>> {
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

    pub fn known(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
    ) -> anyhow::Result<bool> {
        Ok(self.get2(filter, from)?.is_some())
    }

    pub fn get(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
    ) -> anyhow::Result<Option<gix_hash::ObjectId>> {
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
        from: gix_hash::ObjectId,
    ) -> anyhow::Result<Option<gix_hash::ObjectId>> {
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
            if oid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
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
    fn get_index(&self, tree: gix_hash::ObjectId) -> Option<gix_hash::ObjectId> {
        self.transaction.get_trigram_index(tree, self.hint)
    }

    fn set_index(&self, tree: gix_hash::ObjectId, index: gix_hash::ObjectId) {
        self.transaction
            .insert_trigram_index(tree, index, self.hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_transaction() -> (tempfile::TempDir, Transaction) {
        // These tests exercise ref/commit machinery, not the on-disk cache, so an empty cache
        // stack (no sled backend) is enough.
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context = TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(crate::cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        (dir, transaction)
    }

    fn commit_with_parents(
        transaction: &Transaction,
        msg: &str,
        parents: &[gix_hash::ObjectId],
    ) -> gix_hash::ObjectId {
        let repo = transaction.repo();
        let tree = gix_object::Write::write(
            &repo.objects,
            &gix_object::Tree {
                entries: Vec::new(),
            },
        )
        .unwrap();
        let sig = gix_actor::Signature {
            name: "t".into(),
            email: "t@example.com".into(),
            time: gix_actor::date::Time {
                seconds: 0,
                offset: 0,
            },
        };
        josh_gix_ext::write_commit(&repo.objects, tree, parents, &sig, &sig, msg).unwrap()
    }

    fn commit(transaction: &Transaction, msg: &str) -> gix_hash::ObjectId {
        commit_with_parents(transaction, msg, &[])
    }

    fn disk_ref(path: &std::path::Path, name: &str) -> Option<gix_hash::ObjectId> {
        gix::open(path)
            .unwrap()
            .try_find_reference(name)
            .unwrap()
            .and_then(|reference| reference.target().try_id().map(ToOwned::to_owned))
    }

    fn disk_blob(path: &std::path::Path, oid: gix_hash::ObjectId) -> Option<Vec<u8>> {
        let repo = gix::open(path).unwrap();
        repo.find_object(oid)
            .ok()
            .filter(|object| object.kind == gix_object::Kind::Blob)
            .map(|object| object.data.clone())
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
            .create_symref("refs/josh/sym", "refs/heads/main", "test")
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
            .create_symref("refs/josh/dangling", "refs/heads/missing", "test")
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
    fn update_ref_absent_fails_when_pending_refs_are_written() {
        let (dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        transaction.apply_pending_refs().unwrap();

        transaction
            .update_ref("refs/heads/main", Expected::Absent, b, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
        assert!(transaction.apply_pending_refs().is_err());

        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(a));
        assert_eq!(disk_ref(dir.path(), "refs/heads/main"), Some(a));
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
        transaction.apply_pending_refs().unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
    }

    #[test]
    fn update_ref_at_fails_on_mismatch_when_pending_refs_are_written() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        let c = commit(&transaction, "c");
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        transaction
            .update_ref("refs/heads/main", Expected::At(b), c, "test")
            .unwrap();
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(c));
        assert!(transaction.apply_pending_refs().is_err());
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(a));
    }

    #[test]
    fn update_ref_at_fails_on_missing_ref_when_pending_refs_are_written() {
        let (_dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/missing", Expected::At(a), b, "test")
            .unwrap();
        assert_eq!(
            transaction.resolve_ref("refs/heads/missing").unwrap(),
            Some(b)
        );
        assert!(transaction.apply_pending_refs().is_err());
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
        // Write a packed ref directly so the CAS must find it there.
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
        transaction.apply_pending_refs().unwrap();
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
        // A ref in a subdirectory beside one whose name sorts around it: `-` is below `/`,
        // so a walk that treats a directory as a plain name orders these the other way.
        transaction
            .update_ref("refs/josh/c/d", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .update_ref("refs/josh/c-d", Expected::Any, oid, "test")
            .unwrap();
        transaction.apply_pending_refs().unwrap();
        transaction
            .create_symref("refs/josh/aa", "refs/josh/a", "test")
            .unwrap();
        transaction.apply_pending_refs().unwrap();

        // Pack `refs/josh/a` by hand (no pack-refs API is exposed), so the listing also has
        // to merge the packed side in.
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
        assert_eq!(
            seen,
            [
                "refs/josh/a",
                "refs/josh/b",
                "refs/josh/c-d",
                "refs/josh/c/d"
            ]
        );
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
    fn for_each_ref_prefixed_takes_a_partial_component() {
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        for name in [
            "refs/heads/feature",
            "refs/heads/featherweight",
            "refs/heads/x",
        ] {
            transaction
                .update_ref(name, Expected::Any, oid, "test")
                .unwrap();
        }

        let mut seen = vec![];
        transaction
            .for_each_ref_prefixed("refs/heads/feat", |name, _| {
                seen.push(name.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, ["refs/heads/featherweight", "refs/heads/feature"]);
    }

    #[test]
    fn for_each_ref_prefixed_without_a_slash_lists_every_matching_ref() {
        // A user-supplied glob's literal part can be a prefix that names no directory --
        // `` or `refs` -- which is where the directory walk cannot be used.
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/heads/main", Expected::Any, oid, "test")
            .unwrap();

        for prefix in ["", "refs"] {
            let mut seen = vec![];
            transaction
                .for_each_ref_prefixed(prefix, |name, _| {
                    seen.push(name.to_owned());
                    Ok(())
                })
                .unwrap();
            assert_eq!(seen, ["refs/heads/main"], "prefix '{}'", prefix);
        }

        let mut seen = vec![];
        transaction
            .for_each_ref_prefixed("other", |name, _| {
                seen.push(name.to_owned());
                Ok(())
            })
            .unwrap();
        assert!(seen.is_empty());
    }

    // Windows cannot hold this ref at all: a device name is illegal as a path component, so
    // an upstream whose path contains one cannot be mirrored there.
    #[cfg(unix)]
    #[test]
    fn for_each_ref_prefixed_takes_a_prefix_no_worktree_could_hold() {
        // Upstream namespaces are percent-encoded repository paths, so a prefix component
        // can be a name gix refuses as a relative path -- here a Windows device name.
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        let prefix = "refs/josh/upstream/aux/refs/heads/";
        transaction
            .update_ref(&format!("{}main", prefix), Expected::Any, oid, "test")
            .unwrap();
        transaction
            .update_ref("refs/heads/main", Expected::Any, oid, "test")
            .unwrap();

        let mut seen = vec![];
        transaction
            .for_each_ref_prefixed(prefix, |name, _| {
                seen.push(name.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, ["refs/josh/upstream/aux/refs/heads/main"]);
    }

    // Identifies the file by inode, so it only makes sense on unix.
    #[cfg(unix)]
    #[test]
    fn update_ref_to_the_value_a_ref_already_has_writes_nothing() {
        use std::os::unix::fs::MetadataExt;
        let (dir, transaction) = test_transaction();
        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        transaction.apply_pending_refs().unwrap();

        // A write is a lock file and a rename, so it replaces the file; leaving the ref
        // alone keeps it.
        let inode = || dir.path().join("refs/heads/main").metadata().unwrap().ino();
        let before = inode();
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "test")
            .unwrap();
        transaction
            .update_ref("refs/heads/main", Expected::At(a), a, "test")
            .unwrap();
        assert_eq!(inode(), before);

        transaction
            .update_ref("refs/heads/main", Expected::At(b), b, "test")
            .unwrap();
        assert!(transaction.apply_pending_refs().is_err());
        assert_eq!(inode(), before);
        transaction
            .update_ref("refs/heads/main", Expected::Any, b, "test")
            .unwrap();
        transaction.apply_pending_refs().unwrap();
        assert_ne!(inode(), before);
        assert_eq!(transaction.resolve_ref("refs/heads/main").unwrap(), Some(b));
    }

    #[test]
    fn expand_ref_name_resolves_symbolic_refs() {
        // Callers hand the expanded name to `update_ref`, where a guarded write against a
        // symbolic ref could not match an oid.
        let (_dir, transaction) = test_transaction();
        let oid = commit(&transaction, "a");
        transaction
            .update_ref("refs/heads/main", Expected::Any, oid, "test")
            .unwrap();
        transaction
            .create_symref("HEAD", "refs/heads/main", "test")
            .unwrap();

        assert_eq!(
            transaction.expand_ref_name("HEAD").unwrap().as_deref(),
            Some("refs/heads/main")
        );
        assert_eq!(
            transaction.expand_ref_name("main").unwrap().as_deref(),
            Some("refs/heads/main")
        );
        assert_eq!(transaction.expand_ref_name("nope").unwrap(), None);
    }

    #[test]
    fn rev_parse_resolves_revision_syntax_and_rejects_user_input() {
        let (_dir, transaction) = test_transaction();
        let parent = commit(&transaction, "parent");
        let tip = commit_with_parents(&transaction, "tip", &[parent]);
        transaction
            .update_ref("refs/heads/main", Expected::Any, tip, "test")
            .unwrap();
        transaction.apply_pending_refs().unwrap();

        assert_eq!(transaction.rev_parse("main").unwrap(), Some(tip));
        assert_eq!(transaction.rev_parse("main~1").unwrap(), Some(parent));
        assert_eq!(transaction.rev_parse("missing").unwrap(), None);
        assert_eq!(transaction.rev_parse("main~nope").unwrap(), None);
        assert_eq!(transaction.rev_parse("main..main").unwrap(), None);
    }

    #[test]
    fn configuration_methods_use_repository_configuration() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("config"))
            .unwrap()
            .write_all(
                br#"
[user]
    name = Configured User
    email = configured@example.com
[branch "main"]
    remote = origin
    merge = refs/heads/main
[remote "origin"]
    fetch = +refs/heads/*:refs/remotes/origin/*
"#,
            )
            .unwrap();

        let context = TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(crate::cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();

        assert_eq!(
            transaction.config_string("user.email").unwrap().as_deref(),
            Some("configured@example.com")
        );
        assert_eq!(
            transaction
                .upstream_ref("refs/heads/main")
                .unwrap()
                .as_deref(),
            Some("refs/remotes/origin/main")
        );
        assert_eq!(transaction.upstream_ref("refs/heads/other").unwrap(), None);
        let signature = transaction.signature().unwrap();
        assert_eq!(signature.name.as_slice(), b"Configured User");
        assert_eq!(signature.email.as_slice(), b"configured@example.com");
    }

    #[test]
    fn update_ref_writes_a_reflog_entry() {
        // The one thing a bare test repository cannot show: gix appends to the reflog where
        // git does, and refuses to do it without a committer.
        let dir = tempfile::tempdir().unwrap();
        gix::init(dir.path()).unwrap();
        let context = TransactionContext::new(
            dir.path().join(".git"),
            std::sync::Arc::new(crate::cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();

        let a = commit(&transaction, "a");
        let b = commit(&transaction, "b");
        transaction
            .update_ref("refs/heads/main", Expected::Any, a, "first")
            .unwrap();
        transaction
            .update_ref("refs/heads/main", Expected::Any, b, "second")
            .unwrap();
        transaction.apply_pending_refs().unwrap();

        let log = std::fs::read_to_string(dir.path().join(".git/logs/refs/heads/main")).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 2, "{}", log);
        assert!(lines[0].starts_with(&format!(
            "{} {}",
            gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            a
        )));
        assert!(lines[0].ends_with("\tfirst"));
        assert!(lines[1].starts_with(&format!("{} {}", a, b)));
        assert!(lines[1].ends_with("\tsecond"));
    }

    #[test]
    fn reflog_committer_falls_back_to_unknown() {
        let committer = reflog_committer(None);
        assert_eq!(committer.name, "unknown");
        assert_eq!(committer.email, "unknown");
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
        // Write a packed ref directly so deletion must find it there.
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
        assert!(disk_ref(transaction.path(), "refs/josh/sym").is_none());
        assert_eq!(
            transaction.resolve_ref("refs/heads/main").unwrap(),
            Some(oid)
        );
    }

    /// A context the store-lifetime tests below open several transactions from in turn.
    fn test_context() -> (tempfile::TempDir, TransactionContext) {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
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
            assert!(disk_ref(dir.path(), "refs/josh/blob").is_none());
            assert!(disk_blob(dir.path(), oid).is_none());
            oid
        };

        assert_eq!(disk_ref(dir.path(), "refs/josh/blob"), Some(oid));
        assert_eq!(
            disk_blob(dir.path(), oid).as_deref(),
            Some(b"published".as_slice())
        );
    }

    #[test]
    fn failed_object_flush_does_not_publish_ref() {
        let (dir, context) = test_context();
        {
            let transaction = context.open().unwrap();
            let oid = transaction
                .odb()
                .write(gix_object::Kind::Blob, b"cannot publish");
            transaction
                .update_ref("refs/josh/blob", Expected::Absent, oid, "test")
                .unwrap();
            let pack_dir = dir.path().join("objects").join("pack");
            std::fs::remove_dir(&pack_dir).unwrap();
            std::fs::write(pack_dir, b"not a directory").unwrap();
        }

        assert!(!dir.path().join("refs/josh/blob").exists());
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

        assert_eq!(disk_ref(dir.path(), "refs/josh/blob"), Some(oid));
        assert_eq!(
            disk_blob(dir.path(), oid).as_deref(),
            Some(b"boundary".as_slice())
        );
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
            assert!(disk_ref(dir.path(), "refs/josh/ephemeral").is_none());
        }

        assert!(disk_ref(dir.path(), "refs/josh/ephemeral").is_none());
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
