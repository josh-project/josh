use super::cache::{CACHE_VERSION, CacheBackend};
use crate::JoshResult;
use crate::filter;
use crate::filter::Filter;

pub struct NotesCacheBackend {
    repo: std::sync::Mutex<git2::Repository>,
}

impl NotesCacheBackend {
    pub fn new(repo_path: impl AsRef<std::path::Path>) -> JoshResult<Self> {
        let repo = git2::Repository::open(repo_path.as_ref())?;
        Ok(Self {
            repo: std::sync::Mutex::new(repo),
        })
    }
}

// The notes cache is meant to be sparse. That is, not all entries are actually persisted.
// This makes it smaller and faster to download.
// It is expected that on any node (server, proxy, local repo) a full "dense" local cache
// is used in addition to the sparse note cache.
// The note cache is mostly only used for initial "cold starts" or longer "catch up".
// For incremental filtering it's fine re-filter commits and rely on the local "dense" cache.
// We store entries for 1% of all commits, and additionally all merges and orphans.
fn is_note_eligible(repo: &git2::Repository, oid: git2::Oid, sequence_number: u128) -> bool {
    let parent_count = if let Ok(c) = repo.find_commit(oid) {
        c.parent_ids().count()
    } else {
        return false;
    };

    sequence_number % 100 == 0 || parent_count != 1
}

// To additionally limit the size of the note trees the cache is also sharded by sequence
// number in groups of 10000. Note that this does not limit the number of entried per bucket
// as branches mean many commits share the same sequence number.
fn note_path(key: git2::Oid, sequence_number: u128) -> String {
    format!(
        "refs/josh/{}/{}/{}",
        CACHE_VERSION,
        sequence_number / 10000,
        key,
    )
}

impl CacheBackend for NotesCacheBackend {
    fn read(
        &self,
        filter: Filter,
        from: git2::Oid,
        sequence_number: u128,
    ) -> JoshResult<Option<git2::Oid>> {
        if filter == filter::sequence_number() {
            return Ok(None);
        }
        let repo = self.repo.lock()?;
        if !is_note_eligible(&repo, from, sequence_number) {
            return Ok(None);
        }

        let key = filter.id();

        if let Ok(note) = repo.find_note(Some(&note_path(key, sequence_number)), from) {
            let message = note.message().unwrap_or("");
            let result = git2::Oid::from_str(message)?;

            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn write(
        &self,
        filter: Filter,
        from: git2::Oid,
        to: git2::Oid,
        sequence_number: u128,
    ) -> JoshResult<()> {
        if filter == filter::sequence_number() {
            return Ok(());
        }

        let repo = self.repo.lock()?;
        if !is_note_eligible(&*repo, from, sequence_number) {
            return Ok(());
        }

        let key = filter.id();
        let signature = super::cache::josh_commit_signature()?;

        repo.note(
            &signature,
            &signature,
            Some(&note_path(key, sequence_number)),
            from,
            &to.to_string(),
            true,
        )?;

        Ok(())
    }
}
