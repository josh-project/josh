use super::transaction::{CACHE_VERSION, CacheBackend};
use crate::filter;
use crate::filter::Filter;
use std::collections::HashMap;

// Only flush shards after they gained enough new entries
const FLUSH_AFTER: usize = 1000;

pub struct DistributedCacheBackend {
    new_entries: std::sync::Mutex<HashMap<(Filter, u128), HashMap<git2::Oid, git2::Oid>>>,
    repo: std::sync::Mutex<git2::Repository>,
}

impl Drop for DistributedCacheBackend {
    fn drop(&mut self) {
        if !self.flush(true).is_ok() {
            log::warn!("DistributedCacheBackend: flush failed");
        }
    }
}

impl DistributedCacheBackend {
    pub fn new(repo_path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let repo = git2::Repository::open(repo_path.as_ref())?;
        Ok(Self {
            repo: std::sync::Mutex::new(repo),
            new_entries: Default::default(),
        })
    }

    pub fn flush(&self, force: bool) -> anyhow::Result<()> {
        let repo = self.repo.lock().unwrap();

        let mut guard = self.new_entries.lock().unwrap();

        for ((filter, shard), m) in guard.iter_mut() {
            if !(force || m.len() >= FLUSH_AFTER) {
                continue;
            }
            let rp = ref_path(*filter, *shard);
            let mut builder = git2::build::TreeUpdateBuilder::new();
            builder.upsert(
                "filter",
                crate::filter::as_tree(&repo, *filter)?,
                git2::FileMode::Tree.into(),
            );

            for (from, to) in &mut *m {
                let blob = repo.blob(to.to_string().as_bytes())?;
                builder.upsert(fanout(*from), blob, git2::FileMode::Blob.into());
            }
            let tree = if let Ok(r) = repo.revparse_single(&rp) {
                r.peel_to_tree()?
            } else {
                crate::filter::tree::empty(&repo)
            };
            let updated = builder.create_updated(&repo, &tree)?;

            let signature = crate::git::josh_commit_signature()?;
            let parents = if let Ok(r) = repo.revparse_single(&rp) {
                vec![r.peel_to_commit()?]
            } else {
                vec![]
            };
            let parent_refs = parents.iter().collect::<Vec<_>>();

            let _ = repo.commit(
                Some(&rp),
                &signature,
                &signature,
                "cache",
                &repo.find_tree(updated)?,
                &parent_refs,
            )?;
            log::info!("CACHE flush {} {}", m.len(), rp);
            m.clear();
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
fn is_eligible(repo: &git2::Repository, oid: git2::Oid, sequence_number: u128) -> bool {
    let parent_count = if let Ok(c) = repo.find_commit(oid) {
        c.parent_ids().count()
    } else {
        return false;
    };

    sequence_number % 100 == 0 || parent_count != 1
}

// To additionally limit the size of the trees the cache is also sharded by sequence
// number in groups of 10000. Note that this does not limit the number of entries per bucket
// as branches mean many commits share the same sequence number.
fn ref_path(filter: Filter, shard: u128) -> String {
    format!(
        "refs/josh/cache/{}/{}/{}",
        CACHE_VERSION,
        shard,
        filter.id(),
    )
}

fn fanout(commit: git2::Oid) -> std::path::PathBuf {
    let commit = commit.to_string();
    std::path::Path::new(&commit[..2])
        .join(&commit[2..5])
        .join(&commit[5..9])
        .join(commit)
}

impl CacheBackend for DistributedCacheBackend {
    fn read(
        &self,
        filter: Filter,
        from: git2::Oid,
        sequence_number: u128,
    ) -> anyhow::Result<Option<git2::Oid>> {
        if filter == filter::sequence_number() {
            return Ok(None);
        }
        let repo = self.repo.lock().unwrap();
        if !is_eligible(&repo, from, sequence_number) {
            return Ok(None);
        }

        let guard = self.new_entries.lock().unwrap();

        // See if this is one of the newly added entries first
        let shard = sequence_number / 10000;
        if let Some(shard_map) = guard.get(&(filter, shard))
            && let Some(to) = shard_map.get(&from)
        {
            return Ok(Some(*to));
        }

        std::mem::drop(guard);

        let rp = ref_path(filter, shard);
        let tree = if let Ok(r) = repo.revparse_single(&rp) {
            r.peel_to_tree()?
        } else {
            return Ok(None);
        };

        if let Ok(e) = tree.get_path(&fanout(from)) {
            let blob = repo.find_blob(e.id())?;
            let s = std::str::from_utf8(blob.content())?.to_owned();
            log::debug!(
                "DistributedCacheBackend: HIT {:?} {}",
                from,
                filter::spec(filter)
            );
            return Ok(Some(git2::Oid::from_str(&s)?));
        } else {
            return Ok(None);
        };
    }

    fn write(
        &self,
        filter: Filter,
        from: git2::Oid,
        to: git2::Oid,
        sequence_number: u128,
    ) -> anyhow::Result<()> {
        if filter == filter::sequence_number() {
            return Ok(());
        }

        let repo = self.repo.lock().unwrap();
        if !is_eligible(&repo, from, sequence_number) {
            return Ok(());
        }

        let shard = sequence_number / 10000;

        let mut guard = self.new_entries.lock().unwrap();

        let shard_map = guard.entry((filter, shard)).or_insert(Default::default());

        shard_map.insert(from, to);

        if shard_map.len() < FLUSH_AFTER {
            return Ok(());
        }

        std::mem::drop(guard);
        std::mem::drop(repo);

        self.flush(false)?;

        Ok(())
    }
}
