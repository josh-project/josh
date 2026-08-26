use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::Filter;
use josh_core::git::josh_actor_signature;
use josh_test_support::bench::{EntryKind, build_index, random_string};
use rand::prelude::*;
use std::path::{Path, PathBuf};
use std::str::FromStr;

// This bench measures the object *write* path: unlike `:/<subdir>` (which only selects existing
// subtrees, so the filtered output reuses existing tree objects), a `:prefix=<path>` filter writes
// fresh objects for every filtered commit -- one new tree per prefix component plus the rewritten
// commit itself. All of those writes land in the per-transaction in-memory object store
// (josh-memodb) and are packed to disk by its background flusher, so this bench covers both the
// per-object write cost and the overflow-pack/boundary-drain cost that the other benches only
// exercise incidentally. The mem-odb size limit is set artificially low (see `MEM_ODB_LIMIT`) so
// mid-transaction overflow packs actually trigger at bench scale, and the timed section ends with
// an explicit boundary flush so pack durability is inside the measurement.

// Number of history commits generated on top of the root commit for each case. Kept small in debug
// builds so `cargo test`/`--test` runs stay fast.
const HISTORY_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[10, 100]
} else {
    &[100, 1_000]
};

// A fixed, modest tree shared as the root of every case, same shape as `deephistory_subdir`: this
// bench varies history length, not tree width.
const TREE_FILES: usize = 200;
const N_DIRS: usize = 10;

// Fraction of the tree's files each commit changes ("churns"). Churn makes every commit's root tree
// unique, so the prefix filter writes distinct trees for every commit instead of hitting the odb
// dedup path.
const CHURN_FRACTION: f64 = 0.1;
const CHURN_CONTENT_LEN: usize = 10;

// The prefix the benchmarked filter applies. Three components deep, so every filtered commit writes
// three fresh tree objects plus the commit object.
const PREFIX: &str = "a/b/c";

// In-memory object store size limit for the benchmarked transactions. Low enough that the writes of
// the larger cases exceed it several times over, forcing the mid-transaction overflow packs this
// bench exists to measure; high enough that the smallest case still fits (giving one boundary-drain
// data point without overflow packs).
const MEM_ODB_LIMIT: usize = 64 * 1024;

/// Expected oid of the cached bench repo's aggregate index commit. This is the cache validity key:
/// changing any build parameter above changes a case head, which changes the index commit oid,
/// which then fails the strict check in `provision_repo` and reports the new value to paste here.
/// Filled in by running the bench once after a build change. Debug builds use reduced
/// HISTORY_SIZES, so they have their own expected oid and their own provision-cache name (below)
/// -- otherwise `cargo test --benches` and `cargo bench` would fight over the same cache entry.
const EXPECTED_HEAD: &str = if cfg!(debug_assertions) {
    "d4c3f665871dd8f4413cae2aa55a73f4d86d8ccc"
} else {
    "85136c91e53cddde412086e85d25a33d05931ff0"
};

/// Provision-cache name, split per profile to match the per-profile EXPECTED_HEAD.
const CACHE_NAME: &str = if cfg!(debug_assertions) {
    "deephistory_prefix_flush_debug"
} else {
    "deephistory_prefix_flush"
};

/// Fixed commit timestamp fed to `josh_actor_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces
/// different head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One history length and the head of its generated history.
struct SizeCase {
    n_commits: usize,
    head: gix_hash::ObjectId,
}

struct PrefixFlushBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark.
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
    // The filter under benchmark: a single `:prefix=<PREFIX>`.
    filter: Filter,
}

impl PrefixFlushBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oids are reproducible and
        // `EXPECTED_HEAD` stays valid across runs. Must run before the cache-miss path invokes the
        // build callback. SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        // Build (or reuse from cache) the bare repo holding every history-length case. On a cache
        // miss the callback builds all cases, tags each tip with a `refs/heads/case_<n_commits>`
        // ref, and returns an aggregate index commit whose oid is the content-addressed cache
        // stamp checked against `EXPECTED_HEAD`.
        let provisioned = josh_test_support::provision_repo::provision_repo(
            CACHE_NAME,
            &gix_hash::ObjectId::from_str(EXPECTED_HEAD)
                .expect("EXPECTED_HEAD must be a valid oid"),
            |repo| {
                let mut heads = vec![];
                for &n_commits in HISTORY_SIZES {
                    let head = tracing::info_span!(target: "bench", "build_case", n_commits)
                        .in_scope(|| build_case(repo, n_commits))?;
                    heads.push(head);
                }
                build_index(repo, &josh_actor_signature()?, &heads)
            },
        )?;

        // Recover each case head from its ref. This runs identically whether the repo was freshly
        // built or copied from cache.
        let mut cases = vec![];
        {
            let repo = &provisioned.repo;
            for &n_commits in HISTORY_SIZES {
                let head = josh_test_support::bench::ref_target(
                    repo,
                    &format!("refs/heads/case_{n_commits}"),
                )?;
                cases.push(SizeCase {
                    n_commits,
                    head: head,
                });
            }
        }

        let filter = Filter::new().prefix(PREFIX);

        // Pin the sled db open across iterations: per-iteration transaction drops would
        // otherwise close and reopen it, and the cycle's flush/reopen I/O dominates short cases.
        let sled = josh_core::cache::SledCacheBackend::new(provisioned.path());
        sled.pin()?;
        let cache = std::sync::Arc::new(josh_core::cache::CacheStack::new().with_backend(sled));
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache)
            .with_mem_odb_limit(MEM_ODB_LIMIT);

        // Correctness gate (untimed): confirm the prefix filter nests the raw head tree under
        // `PREFIX`, so we never silently measure a filter that drops everything or is a no-op. Run
        // through a throwaway transaction on the smallest case (the check is history-length
        // independent), then reset caches so nothing here warms the timed runs.
        {
            let transaction = context.open()?;
            let case = cases.first().expect("at least one case");
            let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
            // The filtered commit is still buffered, so read it through the transaction's
            // object source rather than the repository handle.
            let odb = transaction.odb();
            let filtered_tree = josh_core::objects::CommitData::read(odb, filtered)?.tree_id()?;
            let nested_tree =
                josh_core::objects::path_entry(odb, filtered_tree, Path::new(PREFIX))?
                    .map(|entry| entry.oid)
                    .ok_or_else(|| anyhow::anyhow!("prefix filter produced no `{PREFIX}` entry"))?;
            let raw_tree = josh_core::objects::CommitData::read(odb, case.head)?.tree_id()?;
            anyhow::ensure!(
                nested_tree == raw_tree,
                "prefix filter did not nest the tree under `{PREFIX}` -- benchmark would measure \
                 the wrong thing"
            );
        }
        josh_core::reset_caches()?;

        Ok(Self {
            _repo: provisioned,
            context,
            cases,
            filter,
        })
    }
}

/// Build a root commit whose tree holds `TREE_FILES` files spread across `N_DIRS` top-level
/// directories, then generate an `n_commits` history that churns ~`CHURN_FRACTION` of the files per
/// commit. The tip is tagged with `refs/heads/case_<n_commits>` so the head is recoverable after
/// the repo round-trips through the cache.
fn build_case(repo: &gix::Repository, n_commits: usize) -> anyhow::Result<gix_hash::ObjectId> {
    use rand::RngExt;

    let baseline = josh_test_support::bench::empty_tree(repo)?;
    let mut builder = repo.edit_tree(baseline)?;
    let mut all_paths = vec![];
    for i in 0..TREE_FILES {
        let path = PathBuf::from(format!("dir_{:02}", i % N_DIRS)).join(format!("file_{i:04}"));
        let oid = josh_gix_ext::write_blob(&repo.objects, path.to_string_lossy().as_bytes())?;
        builder.upsert(
            path.to_str().expect("benchmark paths are UTF-8"),
            EntryKind::Blob,
            oid,
        )?;
        all_paths.push(path);
    }

    let root_tree = builder.write()?.detach();

    let sig = josh_actor_signature()?;
    // No ref update yet -- the tip ref is set once the history is complete.
    let mut head =
        josh_gix_ext::write_commit(&repo.objects, root_tree, &[], &sig, &sig, "content")?;

    // Deterministic history: each commit churns a fresh random ~CHURN_FRACTION of the files.
    let mut rng = StdRng::seed_from_u64(1);
    for i in 0..n_commits {
        let parent = josh_gix_ext::CommitData::read(&repo.objects, head)?;
        let tree = parent.tree_id()?;
        let mut builder = repo.edit_tree(tree)?;

        let churned = all_paths
            .iter()
            .filter(|_| rng.random_bool(CHURN_FRACTION))
            .cloned()
            .collect::<Vec<_>>();

        for path in &churned {
            let content = random_string(&mut rng, CHURN_CONTENT_LEN);
            let blob = josh_gix_ext::write_blob(&repo.objects, content.as_bytes())?;
            builder.upsert(
                path.to_str().expect("benchmark paths are UTF-8"),
                EntryKind::Blob,
                blob,
            )?;
        }

        let new_tree = builder.write()?.detach();
        head = josh_gix_ext::write_commit(
            &repo.objects,
            new_tree,
            &[head],
            &sig,
            &sig,
            &format!("commit {i}"),
        )?;
    }

    // Tag the tip so `setup` can find this case's head on the cache-hit path, where the build
    // callback never runs. Also keeps the whole history reachable, so provision_repo's `git prune`
    // retains it.
    repo.reference(
        format!("refs/heads/case_{n_commits}"),
        head,
        gix::refs::transaction::PreviousValue::Any,
        "bench case tip",
    )?;

    Ok(head)
}

fn deephistory_prefix_flush(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = PrefixFlushBench::setup().expect("set up benchmark");

    let mut group = c.benchmark_group("deephistory_prefix_flush");
    // The largest case costs seconds per iteration, so keep Criterion at its minimum sample count
    // to bound the total wall-clock of a run.
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(case.n_commits as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_commits), |b| {
            b.iter_batched(
                // Per-iteration setup (untimed): start from a cold cache and a fresh transaction so
                // every run does the full filtering and object-write work instead of hitting
                // memoized results. The written objects themselves persist in the repo across
                // iterations, but the write path buffers and packs them again regardless, so the
                // measured work is stable.
                || {
                    josh_core::reset_caches().expect("reset caches");
                    let transaction = bench.context.open().expect("open transaction");
                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                    (transaction, iter_span)
                },
                // Timed: filter the case head (writing prefix trees and commits into the mem odb,
                // with overflow packs once MEM_ODB_LIMIT is exceeded), then drain the store so pack
                // durability is part of the measurement.
                |(transaction, iter_span)| {
                    josh_core::filter_commit(&transaction, bench.filter, case.head)
                        .expect("filter commit");
                    transaction.flush_mem_odb().expect("flush mem odb");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, deephistory_prefix_flush);
criterion_main!(benches);
