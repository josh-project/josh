//! `Transaction::release_cache` must free sled's exclusive lock on the cache directory so another
//! opener can take it. This lives in its own integration binary because it loads and unloads the
//! process-global cache db; sharing that global with the unit tests (which keep it loaded for the
//! whole run) would race.

use std::sync::Arc;

use josh_core::cache::{CacheStack, TransactionContext, sled_load, sled_unload};

#[test]
fn release_cache_frees_the_sled_lock() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git2::Repository::init_bare(repo).unwrap();

    sled_load(repo).unwrap();
    let transaction = TransactionContext::new(repo, Arc::new(CacheStack::default()))
        .open()
        .unwrap();

    // Populate a cache tree so the transaction and the backend both hold live sled handles.
    transaction.insert_paths((git2::Oid::ZERO_SHA1, "x".into()), git2::Oid::ZERO_SHA1);

    // A second open of the same cache dir must fail while the transaction holds the lock: sled_load
    // opens before swapping the global db in, so it observes the still-held lock and errors.
    assert!(
        sled_load(repo).is_err(),
        "cache should be locked while the transaction holds it"
    );

    transaction.release_cache().unwrap();

    // With every sled handle dropped, the cache dir opens cleanly again.
    sled_load(repo).expect("cache still locked after release_cache");
    sled_unload();
}
