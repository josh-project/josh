//! `Transaction::release_cache` must free sled's exclusive lock on the cache directory so another
//! opener can take it. This lives in its own integration binary because it loads and unloads the
//! process-global cache db; sharing that global with the unit tests (which keep it loaded for the
//! whole run) would race.

use std::sync::Arc;

use josh_core::cache::{CacheStack, TransactionContext, sled_load, sled_unload};
use josh_core::filter;

#[test]
fn release_cache_frees_the_sled_lock() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git2::Repository::init_bare(repo).unwrap();

    sled_load(repo).unwrap();
    let transaction = TransactionContext::new(repo, Arc::new(CacheStack::default()))
        .open()
        .unwrap();

    // Force a backend cache write so the SledCacheBackend opens a per-filter tree -- that handle,
    // plus the global db, is what release_cache has to drop to free the lock.
    let git = transaction.repo();
    let sig = git2::Signature::new("t", "t@example.com", &git2::Time::new(0, 0)).unwrap();
    let tree = git
        .find_tree(git.treebuilder(None).unwrap().write().unwrap())
        .unwrap();
    let commit = git.commit(None, &sig, &sig, "c", &tree, &[]).unwrap();
    transaction
        .insert(filter::parse(":/").unwrap(), commit, commit, true)
        .unwrap();

    // A second open of the same cache dir must fail while those handles are live: sled_load opens
    // before swapping the global db in, so it observes the still-held lock and errors.
    assert!(
        sled_load(repo).is_err(),
        "cache should be locked while the transaction holds it"
    );

    transaction.release_cache().unwrap();

    // With every sled handle dropped, the cache dir opens cleanly again.
    sled_load(repo).expect("cache still locked after release_cache");
    sled_unload();
}
