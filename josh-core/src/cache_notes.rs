use crate::JoshResult;
use crate::cache::{CACHE_VERSION, CacheBackend};
use crate::filter::Filter;
use crate::filter::n_parents_f;

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

fn is_note_eligible(repo: &git2::Repository, oid: git2::Oid, np: u128) -> bool {
    /* oid.as_bytes()[0] % 8 == 0 */
    let parent_count = if let Ok(c) = repo.find_commit(oid) {
        c.parent_ids().count()
    } else {
        return false;
    };

    np % 100 == 0 || parent_count != 1
    /* || (np+1) % 100 == 0 */
}

fn note_path(key: git2::Oid, np: u128) -> String {
    format!("refs/josh/{}/{}/{}", CACHE_VERSION, key, np / 10000)
}

impl CacheBackend for NotesCacheBackend {
    fn read(&self, filter: Filter, from: git2::Oid, np: u128) -> JoshResult<Option<git2::Oid>> {
        if filter == n_parents_f() {
            return Ok(None);
        }
        let repo = self.repo.lock()?;
        if !is_note_eligible(&repo, from, np) {
            return Ok(None);
        }

        let key = crate::filter::as_tree(&*repo, filter)?;

        if let Ok(note) = repo.find_note(Some(&note_path(key, np)), from) {
            let message = note.message().unwrap_or("");
            let result = git2::Oid::from_str(message)?;

            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn write(&self, filter: Filter, from: git2::Oid, to: git2::Oid, np: u128) -> JoshResult<()> {
        if filter == n_parents_f() {
            return Ok(());
        }

        let repo = self.repo.lock()?;
        if !is_note_eligible(&*repo, from, np) {
            return Ok(());
        }

        let key = crate::filter::as_tree(&*repo, filter)?;
        let signature = crate::cache::josh_commit_signature()?;

        repo.note(
            &signature,
            &signature,
            Some(&note_path(key, np)),
            from,
            &to.to_string(),
            true,
        )?;

        Ok(())
    }
}
