//! Helpers for locating on-disk packfile storage within a repository.

use std::path::PathBuf;

/// The directory where `repo`'s packfiles live (`objects/pack`).
///
/// Resolved from the repository's *common* directory rather than its gitdir: a linked worktree has
/// no `objects/` of its own, so its packs live under the common dir. For a non-worktree repo the two
/// are the same.
pub fn packfile_path(repo: &git2::Repository) -> PathBuf {
    repo.commondir().join("objects").join("pack")
}
