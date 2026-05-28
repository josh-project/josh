use git2::Repository;
use std::path::Path;

pub fn open_repo(path: impl AsRef<Path>) -> Result<Repository, git2::Error> {
    Repository::discover(path)
}

pub fn resolve_commit(repo: &Repository, spec: Option<&str>) -> Result<git2::Oid, git2::Error> {
    if let Some(spec) = spec {
        repo.revparse_single(spec)?.peel_to_commit().map(|c| c.id())
    } else {
        repo.head()?.resolve()?.peel_to_commit().map(|c| c.id())
    }
}
