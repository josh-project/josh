//! Cache expensive benchmark-repo builds on disk so they are rebuilt from scratch
//! at most once per build configuration.
//!
//! See [`provision_repo`] for the entry point.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

/// A benchmark repo provisioned into a throwaway tempdir.
///
/// The canonical form is cached under the user's cache directory; this handle
/// owns a fresh copy in a tempdir together with the [`git2::Repository`] opened
/// from it. Drop the value to delete the tempdir.
pub struct ProvisionedRepo {
    /// Keeps the on-disk tempdir alive for as long as the repo handle is used.
    _tmp: tempfile::TempDir,
    /// A bare repo opened from the tempdir copy.
    pub repo: git2::Repository,
    /// The head oid the callback produced (always equal to the `expected` that
    /// was passed to [`provision_repo`]).
    pub head: git2::Oid,
}

impl ProvisionedRepo {
    /// Path of the bare repo, which for a bare repo is the tempdir itself.
    pub fn path(&self) -> &Path {
        self.repo.path()
    }
}

/// Provision a benchmark repo for `testcase`, caching the canonical build under
/// `<cache_dir>/josh/benches/<testcase>`.
///
/// If the cache already contains the `expected` object, the cached bare repo is
/// copied into a fresh tempdir and returned. Otherwise a new bare repo is built
/// by invoking `callback` (erasing any stale cache), repacked into a single
/// packfile, verified to produce `expected`, cached, and copied into a tempdir.
///
/// `expected` is a content-addressed version stamp: changing the build (file
/// counts, history length, etc.) changes the head oid the callback returns,
/// which no longer matches `expected`, so the strict check fires and the cache
/// is treated as invalid. On the first run after such a change, the error
/// message reports the oid the callback produced so it can be pasted in as the
/// new `expected`.
pub fn provision_repo<C>(
    testcase: &str,
    expected: &git2::Oid,
    callback: C,
) -> Result<ProvisionedRepo>
where
    C: FnMut(&git2::Repository) -> Result<git2::Oid>,
{
    let expected = *expected;
    let cache_root = cache_root_for(testcase)?;

    if cache_hit(&cache_root, expected) {
        return copy_to_tempdir(&cache_root, expected);
    }

    let mut callback = callback;
    rebuild(&cache_root, expected, &mut callback)?;
    copy_to_tempdir(&cache_root, expected)
}

fn cache_root_for(testcase: &str) -> Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow!("no cache directory available on this platform"))?;
    Ok(cache_dir.join("josh").join("benches").join(testcase))
}

/// A cached repo is reusable if it opens as a bare repo and contains `expected`.
fn cache_hit(cache_root: &Path, expected: git2::Oid) -> bool {
    let Ok(repo) = git2::Repository::open_bare(cache_root) else {
        return false;
    };
    repo.odb().map(|odb| odb.exists(expected)).unwrap_or(false)
}

/// Build `testcase` from scratch into `cache_root`, erasing any prior cache.
fn rebuild<C>(cache_root: &Path, expected: git2::Oid, callback: &mut C) -> Result<()>
where
    C: FnMut(&git2::Repository) -> Result<git2::Oid>,
{
    if cache_root.exists() {
        std::fs::remove_dir_all(cache_root)
            .with_context(|| format!("removing stale cache at {}", cache_root.display()))?;
    }
    if let Some(parent) = cache_root.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating cache parent {}", parent.display()))?;
    }

    let repo = git2::Repository::init_bare(cache_root)
        .with_context(|| format!("initializing bare repo at {}", cache_root.display()))?;

    let produced = callback(&repo)?;

    // Pack everything into a single packfile so the tempdir copy is cheap, then
    // drop unreachable loose objects the pack did not absorb (e.g. the empty-tree
    // baseline a treebuilder may leave behind).
    let status = Command::new("git")
        .arg("-C")
        .arg(cache_root)
        .args(["repack", "-a", "-d"])
        .status()
        .context("spawning git repack")?;
    if !status.success() {
        bail!("git repack failed with status {status}");
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(cache_root)
        .args(["prune", "--expire=now"])
        .status()
        .context("spawning git prune")?;
    if !status.success() {
        bail!("git prune failed with status {status}");
    }

    if produced != expected {
        bail!(
            "provision_repo: callback produced head {produced}, \
             but expected {expected}; set EXPECTED to {produced}"
        );
    }

    Ok(())
}

/// Copy the canonical cached repo into a fresh tempdir and open it.
fn copy_to_tempdir(cache_root: &Path, expected: git2::Oid) -> Result<ProvisionedRepo> {
    let tmp = tempfile::tempdir().context("creating tempdir for repo copy")?;
    copy_dir_recursive(cache_root, tmp.path())
        .with_context(|| format!("copying {} to tempdir", cache_root.display()))?;
    let repo = git2::Repository::open_bare(tmp.path()).context("opening copied bare repo")?;
    Ok(ProvisionedRepo {
        _tmp: tmp,
        repo,
        head: expected,
    })
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
