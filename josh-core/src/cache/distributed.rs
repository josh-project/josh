use super::CACHE_VERSION;
use super::backend::{CacheBackend, HistoryGraphHint};
use crate::filter;
use crate::filter::Filter;
use crate::objects;
use std::collections::HashMap;

// Only flush shards after they gained enough new entries. Mid-run flushes enqueue their pack
// work on the background flusher, so a low threshold starts that work early and overlaps it
// with filtering, leaving little for the final forced flush -- which drains sequentially.
const FLUSH_AFTER: usize = 1000;

pub struct DistributedCacheBackend {
    new_entries:
        std::sync::Mutex<HashMap<(Filter, u64), HashMap<gix_hash::ObjectId, gix_hash::ObjectId>>>,
    repo: std::sync::Arc<gix::ThreadSafeRepository>,
    // Whether this backend accepts writes. The default ([`Self::new`]) is read-only: regular
    // sessions consume the fetched cache but should not each grow the shard chains with a
    // commit, pack and ref update for the few entries they produce -- the local sled cache
    // covers those. Only intentional producers (`josh cache build`) open the backend
    // [`Self::writable`].
    writable: bool,
    // In-memory object store, so the trees and commits produced by `flush` are buffered and
    // packed instead of being written synchronously as loose objects.
    mem_odb: std::sync::Arc<josh_memodb::MemOdb>,
    // The facade over `mem_odb` and this repository's objects, held rather than built per call
    // so its store's pack indices are loaded once. Behind a mutex because the facade is not
    // `Sync`.
    odb: std::sync::Mutex<josh_memodb::Odb>,
    // Shard commits built by non-forced flushes, keyed by ref name. Their objects may still be
    // in `mem_odb` only, so the refs are published exclusively by a forced flush, after a drain
    // has made every buffered object durable: a ref on disk must never point to objects that
    // only exist in memory.
    pending_refs: std::sync::Mutex<HashMap<String, gix_hash::ObjectId>>,
    // Filter -> persisted tree id (`as_tree`), used to name cache refs. `as_tree` resolves
    // insert OIDs, so ref names always reference persisted, reachable filter trees even when
    // the filter passed in still contains unresolved ones.
    tree_ids: std::sync::Mutex<HashMap<Filter, gix_hash::ObjectId>>,
}

impl Drop for DistributedCacheBackend {
    fn drop(&mut self) {
        if let Err(error) = self.flush(true) {
            log::warn!("DistributedCacheBackend: flush failed: {error}");
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
        let repo = std::sync::Arc::new(gix::ThreadSafeRepository::open_opts(
            repo_path.as_ref(),
            gix::open::Options::isolated(),
        )?);
        let objects_dir = repo.objects.path().to_owned();
        let mem_odb = josh_memodb::MemOdb::new(None, objects_dir.clone());
        let odb = josh_memodb::Odb::at(mem_odb.clone(), &objects_dir)?;
        Ok(Self {
            repo,
            mem_odb,
            odb: std::sync::Mutex::new(odb),
            writable,
            new_entries: Default::default(),
            pending_refs: Default::default(),
            tree_ids: Default::default(),
        })
    }

    fn tree_id(
        &self,
        odb: &josh_memodb::Odb,
        filter: Filter,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        if let Some(oid) = self.tree_ids.lock().unwrap().get(&filter) {
            return Ok(*oid);
        }
        let oid = josh_filter::persist::as_tree(odb, filter)?;
        self.tree_ids.lock().unwrap().insert(filter, oid);
        Ok(oid)
    }

    pub fn flush(&self, force: bool) -> anyhow::Result<()> {
        let repo = self.repo.to_thread_local();
        let odb = self.odb.lock().unwrap();
        let odb = &*odb;

        let mut guard = self.new_entries.lock().unwrap();
        let mut pending = self.pending_refs.lock().unwrap();

        let mut built_any = false;

        for ((filter, shard), m) in guard.iter_mut() {
            if m.is_empty() || !(force || m.len() >= FLUSH_AFTER) {
                continue;
            }
            let rp = ref_path(self.tree_id(odb, *filter)?, *shard);

            // Include earlier unpublished batches.
            let base = if let Some(oid) = pending.get(&rp) {
                Some(*oid)
            } else {
                repo.try_find_reference(&rp)?
                    .and_then(|reference| reference.target().try_id().map(ToOwned::to_owned))
            };

            let mut buf = Vec::new();
            let root = match base {
                Some(commit) => {
                    let tree = objects::CommitData::read(odb, commit)?.tree_id()?;
                    gix_object::FindExt::find_tree(odb, &tree, &mut buf)?.into()
                }
                None => gix_object::Tree::default(),
            };
            let mut editor = gix_object::tree::Editor::new(root, odb, gix_hash::Kind::Sha1);

            // Each entry is a gitlink: the tree entry stores the target oid directly, and git
            // never requires a gitlink target to be present, so no blob objects are needed and
            // push/fetch never tries to transfer the filtered commits the entries point to.
            // `Oid::ZERO_SHA1` ("filters to nothing") cannot be a gitlink -- null oids are invalid
            // in tree entries -- so it is encoded as a blob entry pointing at the empty blob;
            // the entry mode disambiguates on read.
            for (from, to) in &mut *m {
                let (kind, target) = if *to == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    (
                        gix_object::tree::EntryKind::Blob,
                        objects::write_blob(odb, &[])?,
                    )
                } else {
                    (gix_object::tree::EntryKind::Commit, *to)
                };
                editor.upsert(fanout(*from), kind, target)?;
            }

            let updated = editor.write(|tree| {
                gix_object::Write::write(odb, tree)
                    .map_err(|e| anyhow::anyhow!("write cache tree: {e}"))
            })?;

            let signature = crate::git::josh_actor_signature()?;
            let commit = objects::write_commit(
                odb,
                updated,
                base.as_slice(),
                &signature,
                &signature,
                "cache",
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
            repo.edit_reference(gix::refs::transaction::RefEdit {
                change: gix::refs::transaction::Change::Update {
                    log: gix::refs::transaction::LogChange {
                        mode: gix::refs::transaction::RefLog::AndReference,
                        force_create_reflog: false,
                        message: "cache".into(),
                    },
                    expected: gix::refs::transaction::PreviousValue::Any,
                    new: gix::refs::Target::Object(commit),
                },
                name: gix::refs::FullName::try_from(rp.as_str())?,
                deref: false,
            })?;
        }

        Ok(())
    }
}

// This cache is meant to be sparse. That is, not all entries are actually persisted.
// This makes it smaller and faster to download.
// It is expected that on any node (server, proxy, local repo) a full "dense" local cache
// is used in addition to the sparse cache.
// The sparse cache is mostly only used for initial "cold starts" or longer "catch up".
// For incremental filtering it's fine re-filter commits and rely on the local "dense" cache.
// Only the sample points are persisted; see `HistoryGraphHint::is_sample_point` for the
// invariant that bounds how far a walk runs before it hits one.
//
// To additionally limit the size of the trees the cache is also sharded by sequence
// number in groups of 10000. Note that this does not limit the number of entries per bucket
// as branches mean many commits share the same sequence number.
fn ref_path(filter_tree_id: gix_hash::ObjectId, shard: u64) -> String {
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
fn fanout(commit: gix_hash::ObjectId) -> [gix_object::bstr::BString; 3] {
    let commit = commit.to_string();
    [commit[..2].into(), commit[2..5].into(), commit[5..].into()]
}

impl CacheBackend for DistributedCacheBackend {
    fn read(
        &self,
        filter: Filter,
        from: gix_hash::ObjectId,
        hint: HistoryGraphHint,
        tree_keyed: bool,
    ) -> anyhow::Result<Option<gix_hash::ObjectId>> {
        if filter == filter::sequence_number() || filter == filter::reachable_roots() {
            return Ok(None);
        }
        // Tree-keyed records were written by whichever indexing commit was eligible,
        // so gating reads on the reader's eligibility would hide them from the
        // (usually non-sampled) commits that reuse them.
        if !tree_keyed && !hint.is_sample_point() {
            return Ok(None);
        }
        let repo = self.repo.to_thread_local();

        let guard = self.new_entries.lock().unwrap();

        // See if this is one of the newly added entries first
        let shard = hint.sequence_number / 10000;
        if let Some(shard_map) = guard.get(&(filter, shard))
            && let Some(to) = shard_map.get(&from)
        {
            return Ok(Some(*to));
        }

        std::mem::drop(guard);

        let odb = self.odb.lock().unwrap();
        let odb = &*odb;
        let rp = ref_path(self.tree_id(odb, filter)?, shard);
        // Prefer unpublished entries from this process.
        let pending = self.pending_refs.lock().unwrap();
        let tree = if let Some(oid) = pending.get(&rp) {
            objects::CommitData::read(odb, *oid)?.tree_id()?
        } else if let Ok(Some(reference)) = repo.try_find_reference(&rp)
            && let Some(commit) = reference.target().try_id()
        {
            objects::CommitData::read(odb, commit.to_owned())?.tree_id()?
        } else {
            return Ok(None);
        };
        std::mem::drop(pending);

        let mut buf = Vec::new();
        let root = gix_object::FindExt::find_tree_iter(odb, &tree, &mut buf)?;
        let mut entry_buf = Vec::new();
        let entry = root
            .lookup_entry(odb, &mut entry_buf, fanout(from))
            .map_err(|e| anyhow::anyhow!("cache lookup: {e}"))?;
        let Some(e) = entry else {
            return Ok(None);
        };

        log::debug!(
            "DistributedCacheBackend: HIT {:?} {}",
            from,
            filter::spec(filter)
        );
        // Gitlink entries carry the target oid directly; any other mode is the empty-blob
        // encoding of `Oid::ZERO_SHA1` (see `flush`).
        if e.mode.kind() == gix_object::tree::EntryKind::Commit {
            return Ok(Some(e.oid));
        }
        Ok(Some(gix_hash::ObjectId::null(gix_hash::Kind::Sha1)))
    }

    fn write(
        &self,
        filter: Filter,
        from: gix_hash::ObjectId,
        to: gix_hash::ObjectId,
        hint: HistoryGraphHint,
        // Writes stay eligibility-gated for tree-keyed records too: subtrees recur
        // across commits, so a stable subtree is still caught at some sampled commit.
        _tree_keyed: bool,
    ) -> anyhow::Result<()> {
        if !self.writable {
            return Ok(());
        }
        if filter == filter::sequence_number() || filter == filter::reachable_roots() {
            return Ok(());
        }
        if !hint.is_sample_point() {
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
