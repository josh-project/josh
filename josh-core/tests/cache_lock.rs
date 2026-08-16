//! The sled cache holds a process-exclusive lock on the cache directory only while a transaction is
//! active: it is opened lazily on the first transaction and dropped once the last one ends. This
//! lives in its own integration binary because it exercises that process-global lifetime; sharing
//! it with the unit tests (which configure a cache for the whole run) would race.

use std::sync::Arc;

use josh_core::cache::{CACHE_VERSION, CacheStack, SledCacheBackend, TransactionContext};
use josh_core::filter;

/// The directory sled locks for a cache rooted at `repo` (mirrors the backend's own layout).
fn sled_dir(repo: &std::path::Path) -> std::path::PathBuf {
    repo.join(format!("josh/cache/{}/sled/", CACHE_VERSION))
}

/// Try to open the sled db directly. `Err` means the cache directory is still locked by someone
/// else; the db opened here is closed again immediately.
fn open_is_locked(repo: &std::path::Path) -> bool {
    sled::Config::default().path(sled_dir(repo)).open().is_err()
}

#[test]
fn sled_lock_follows_transaction_lifetime() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git2::Repository::init_bare(repo).unwrap();

    let cache = Arc::new(CacheStack::new().with_backend(SledCacheBackend::new(repo)));
    let transaction = TransactionContext::new(repo, cache).open().unwrap();

    // Force a backend cache write so the sled db (and a per-filter tree) is open -- that is what
    // holds the exclusive lock on the cache directory. Scoped so the git borrows of `transaction`
    // end before it is dropped below.
    {
        let git = transaction.repo();
        let sig = git2::Signature::new("t", "t@example.com", &git2::Time::new(0, 0)).unwrap();
        let tree = git
            .find_tree(git.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        let commit = git.commit(None, &sig, &sig, "c", &tree, &[]).unwrap();
        transaction
            .insert(filter::parse(":/").unwrap(), commit, commit, true)
            .unwrap();
    }

    // While the transaction is alive the cache directory is locked: a direct open must fail.
    assert!(
        open_is_locked(repo),
        "cache should be locked while a transaction is active"
    );

    drop(transaction);

    // Dropping the last transaction closes the sled db and releases the lock, so a fresh open
    // succeeds again.
    assert!(
        !open_is_locked(repo),
        "cache still locked after the transaction was dropped"
    );
}
