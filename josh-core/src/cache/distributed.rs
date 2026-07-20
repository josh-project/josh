use super::CACHE_VERSION;
use super::backend::{CacheBackend, HistoryGraphHint};
use crate::filter;
use crate::filter::Filter;
use std::collections::HashMap;

// Only flush shards after they gained enough new entries. Mid-run flushes enqueue their pack
// work on the background flusher, so a low threshold starts that work early and overlaps it
// with filtering, leaving little for the final forced flush -- which drains sequentially.
const FLUSH_AFTER: usize = 1000;

pub struct DistributedCacheBackend {
    new_entries: std::sync::Mutex<HashMap<(Filter, u64), HashMap<git2::Oid, git2::Oid>>>,
    repo: std::sync::Mutex<git2::Repository>,
    // Whether this backend accepts writes. The default ([`Self::new`]) is read-only: regular
    // sessions consume the fetched cache but should not each grow the shard chains with a
    // commit, pack and ref update for the few entries they produce -- the local sled cache
    // covers those. Only intentional producers (`josh cache build`) open the backend
    // [`Self::writable`].
    writable: bool,
    // In-memory object store registered on `repo`, so the trees and commits produced by
    // `flush` are buffered and packed instead of being written synchronously as loose objects.
    mem_odb: std::sync::Arc<josh_memodb::MemOdb>,
    // Shard commits built by non-forced flushes, keyed by ref name. Their objects may still be
    // in `mem_odb` only, so the refs are published exclusively by a forced flush, after a drain
    // has made every buffered object durable: a ref on disk must never point to objects that
    // only exist in memory.
    pending_refs: std::sync::Mutex<HashMap<String, git2::Oid>>,
    // Filter -> persisted tree id (`as_tree`), used to name cache refs. `as_tree` resolves
    // insert OIDs, so ref names always reference persisted, reachable filter trees even when
    // the filter passed in still contains unresolved ones.
    tree_ids: std::sync::Mutex<HashMap<Filter, git2::Oid>>,
}

impl Drop for DistributedCacheBackend {
    fn drop(&mut self) {
        if !self.flush(true).is_ok() {
            log::warn!("DistributedCacheBackend: flush failed");
        }
    }
}

impl DistributedCacheBackend {
    /// Open the backend read-only: cache refs are consulted, but writes are ignored.
    pub fn new(repo_path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        Self::open(repo_path, false)
    }

    /// Open the backend for producing the cache: writes are buffered, flushed in the
    /// background once shards reach [`FLUSH_AFTER`], and everything left is persisted when the
    /// backend drops.
    pub fn writable(repo_path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        Self::open(repo_path, true)
    }

    fn open(repo_path: impl AsRef<std::path::Path>, writable: bool) -> anyhow::Result<Self> {
        let repo = git2::Repository::open(repo_path.as_ref())?;
        let mem_odb = josh_memodb::MemOdb::new(None, repo.path().to_owned());
        mem_odb.register(&repo);
        Ok(Self {
            repo: std::sync::Mutex::new(repo),
            mem_odb,
            writable,
            new_entries: Default::default(),
            pending_refs: Default::default(),
            tree_ids: Default::default(),
        })
    }

    fn tree_id(&self, repo: &git2::Repository, filter: Filter) -> anyhow::Result<git2::Oid> {
        if let Some(oid) = self.tree_ids.lock().unwrap().get(&filter) {
            return Ok(*oid);
        }
        let oid = filter::as_tree(repo, filter)?;
        self.tree_ids.lock().unwrap().insert(filter, oid);
        Ok(oid)
    }

    pub fn flush(&self, force: bool) -> anyhow::Result<()> {
        let repo = self.repo.lock().unwrap();

        let mut guard = self.new_entries.lock().unwrap();
        let mut pending = self.pending_refs.lock().unwrap();

        let mut built_any = false;

        for ((filter, shard), m) in guard.iter_mut() {
            if m.is_empty() || !(force || m.len() >= FLUSH_AFTER) {
                continue;
            }
            let rp = ref_path(self.tree_id(&repo, *filter)?, *shard);
            let mut builder = git2::build::TreeUpdateBuilder::new();

            // Each entry is a gitlink: the tree entry stores the target oid directly, and git
            // never requires a gitlink target to be present, so no blob objects are needed and
            // push/fetch never tries to transfer the filtered commits the entries point to.
            // `Oid::zero()` ("filters to nothing") cannot be a gitlink -- null oids are invalid
            // in tree entries -- so it is encoded as a blob entry pointing at the empty blob;
            // the entry mode disambiguates on read.
            for (from, to) in &mut *m {
                if *to == git2::Oid::zero() {
                    let blob = repo.blob(&[])?;
                    builder.upsert(fanout(*from), blob, git2::FileMode::Blob.into());
                } else {
                    builder.upsert(fanout(*from), *to, git2::FileMode::Commit.into());
                }
            }

            // Base the update on the newest unpublished commit for this ref when one exists:
            // basing on the published tip would drop the entries of earlier unpublished
            // batches.
            let base = if let Some(oid) = pending.get(&rp) {
                Some(repo.find_commit(*oid)?)
            } else if let Ok(r) = repo.revparse_single(&rp) {
                Some(r.peel_to_commit()?)
            } else {
                None
            };
            let tree = match &base {
                Some(commit) => commit.tree()?,
                None => crate::filter::tree::empty(&repo),
            };
            let updated = builder.create_updated(&repo, &tree)?;

            let signature = crate::git::josh_commit_signature()?;
            let parent_refs = base.iter().collect::<Vec<_>>();

            let commit = repo.commit(
                None,
                &signature,
                &signature,
                "cache",
                &repo.find_tree(updated)?,
                &parent_refs,
            )?;
            log::info!("CACHE flush {} {}", m.len(), rp);
            m.clear();
            pending.insert(rp, commit);
            built_any = true;
        }

        if !force {
            // Start packing the new objects without blocking the caller; the refs stay
            // unpublished until a forced flush has drained the store.
            if built_any {
                self.mem_odb.pack_in_background();
            }
            return Ok(());
        }

        if pending.is_empty() {
            return Ok(());
        }

        // Make every buffered object durable, then publish. The drain queues behind any
        // background chunk still in flight, so it returns only once all pending commits are
        // fully on disk.
        self.mem_odb.flush()?;

        for (rp, commit) in pending.drain() {
            repo.reference(&rp, commit, true, "cache")?;
        }

        Ok(())
    }
}

// The cache is meant to be sparse. That is, not all entries are actually persisted.
// This makes it smaller and faster to download.
// It is expected that on any node (server, proxy, local repo) a full "dense" local cache
// is used in addition to the sparse cache.
// The sparse cache is mostly only used for initial "cold starts" or longer "catch up".
// For incremental filtering it's fine re-filter commits and rely on the local "dense" cache.
// We store entries for 1% of all commits, and additionally all merges and orphans.
// The parent count comes from the cached history-graph hint, so this check never
// reads the commit from the ODB.
fn is_eligible(hint: HistoryGraphHint) -> bool {
    hint.sequence_number % 100 == 0 || hint.parent_count != 1
}

// To additionally limit the size of the trees the cache is also sharded by sequence
// number in groups of 10000. Note that this does not limit the number of entries per bucket
// as branches mean many commits share the same sequence number.
fn ref_path(filter_tree_id: git2::Oid, shard: u64) -> String {
    format!(
        "refs/josh/cache/{}/{}/{}",
        CACHE_VERSION, shard, filter_tree_id,
    )
}

// Two fanout levels (~1M buckets) keep per-flush cost proportional to the flush size: subtrees
// stay near-singleton even for shards with tens of thousands of entries, so a flush never
// rewrites subtrees that grow with the accumulated shard. A single 2-hex level goes quadratic on
// dense shards for exactly that reason, while a third level only adds one more tree write per
// entry without making any subtree meaningfully smaller.
fn fanout(commit: git2::Oid) -> std::path::PathBuf {
    let commit = commit.to_string();
    std::path::Path::new(&commit[..2])
        .join(&commit[2..5])
        .join(&commit[5..])
}

impl CacheBackend for DistributedCacheBackend {
    fn read(
        &self,
        filter: Filter,
        from: git2::Oid,
        hint: HistoryGraphHint,
    ) -> anyhow::Result<Option<git2::Oid>> {
        if filter == filter::sequence_number() || filter == filter::reachable_roots() {
            return Ok(None);
        }
        if !is_eligible(hint) {
            return Ok(None);
        }
        let repo = self.repo.lock().unwrap();

        let guard = self.new_entries.lock().unwrap();

        // See if this is one of the newly added entries first
        let shard = hint.sequence_number / 10000;
        if let Some(shard_map) = guard.get(&(filter, shard))
            && let Some(to) = shard_map.get(&from)
        {
            return Ok(Some(*to));
        }

        std::mem::drop(guard);

        let rp = ref_path(self.tree_id(&repo, filter)?, shard);
        // Flushed-but-unpublished entries live in a pending commit (see `flush`), not behind
        // the ref yet; prefer it so in-process reads keep seeing everything ever flushed.
        let pending = self.pending_refs.lock().unwrap();
        let tree = if let Some(oid) = pending.get(&rp) {
            repo.find_commit(*oid)?.tree()?
        } else if let Ok(r) = repo.revparse_single(&rp) {
            r.peel_to_tree()?
        } else {
            return Ok(None);
        };
        std::mem::drop(pending);

        if let Ok(e) = tree.get_path(&fanout(from)) {
            log::debug!(
                "DistributedCacheBackend: HIT {:?} {}",
                from,
                filter::spec(filter)
            );
            // Gitlink entries carry the target oid directly; any other mode is the empty-blob
            // encoding of `Oid::zero()` (see `flush`).
            if e.filemode() == i32::from(git2::FileMode::Commit) {
                return Ok(Some(e.id()));
            }
            return Ok(Some(git2::Oid::zero()));
        } else {
            return Ok(None);
        };
    }

    fn write(
        &self,
        filter: Filter,
        from: git2::Oid,
        to: git2::Oid,
        hint: HistoryGraphHint,
    ) -> anyhow::Result<()> {
        if !self.writable {
            return Ok(());
        }
        if filter == filter::sequence_number() || filter == filter::reachable_roots() {
            return Ok(());
        }
        if !is_eligible(hint) {
            return Ok(());
        }

        let shard = hint.sequence_number / 10000;

        let mut guard = self.new_entries.lock().unwrap();

        let shard_map = guard.entry((filter, shard)).or_insert(Default::default());

        shard_map.insert(from, to);

        if shard_map.len() < FLUSH_AFTER {
            return Ok(());
        }

        std::mem::drop(guard);

        self.flush(false)?;

        Ok(())
    }
}
