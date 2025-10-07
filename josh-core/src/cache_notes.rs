use crate::JoshResult;
use crate::cache::{CACHE_VERSION, CacheBackend};
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

fn is_note_eligible(oid: git2::Oid) -> bool {
    oid.as_bytes()[0] == 0
}

fn note_path(key: git2::Oid) -> String {
    format!("refs/josh/{}/{}", CACHE_VERSION, key)
}

impl CacheBackend for NotesCacheBackend {
    fn read(&self, filter: Filter, from: git2::Oid) -> JoshResult<Option<git2::Oid>> {
        let repo = self.repo.lock()?;
        let key = crate::filter::as_tree(&repo, filter)?;

        if !is_note_eligible(from) {
            return Ok(None);
        }

        if let Ok(note) = repo.find_note(Some(&note_path(key)), from) {
            let message = note.message().unwrap_or("");
            let result = git2::Oid::from_str(message)?;

            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn write(&self, filter: Filter, from: git2::Oid, to: git2::Oid) -> JoshResult<()> {
        let repo = self.repo.lock()?;
        let key = crate::filter::as_tree(&repo, filter)?;

        if !is_note_eligible(from) {
            return Ok(());
        }

        let signature = crate::cache::josh_commit_signature()?;

        repo.note(
            &signature,
            &signature,
            Some(&note_path(key)),
            from,
            &to.to_string(),
            true,
        )?;

        Ok(())
    }
}
