use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::{Filter, RevMatch};
use josh_core::git::josh_actor_signature;
use josh_test_support::bench::EntryKind;
use rand::prelude::*;
use std::path::{Path, PathBuf};
use std::str::FromStr;

// The scaling parameter of this benchmark is history *length*, not tree width. `filter_commit` walks
// and rewrites a commit's whole ancestry, so applying a filter to the head does O(history) work; this
// bench measures how a plain `:/<subdir>` filter scales as the number of commits grows while the tree
// stays a fixed, modest size. It is the long-history counterpart to the `ultrawide_*` benches, which
// hold history short and grow the tree instead.
//
// Number of history commits generated on top of the root commit for each case. Kept small in debug
// builds so `cargo test`/`--test` runs stay fast.
const HISTORY_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[10, 100]
} else {
    &[100, 1_000, 10_000]
};

// A fixed, modest tree shared as the root of every case. This bench varies history length, not tree
// width, so the tree stays small: TREE_FILES files spread evenly across N_DIRS top-level directories
// (`dir_00`..`dir_{N_DIRS-1}`), each holding TREE_FILES / N_DIRS files.
const TREE_FILES: usize = 200;
const N_DIRS: usize = 10;

// The directory the benchmarked filter selects. It always exists in the tree, and churn keeps
// touching it (see `CHURN_FRACTION`) so a meaningful fraction of commits survive history
// simplification and the filtered history stays non-trivial.
const SUBDIR: &str = "dir_00";

// Fraction of the tree's files each commit changes ("churns"). With ~TREE_FILES/N_DIRS files under
// SUBDIR, this keeps most commits touching SUBDIR, so filtering does not collapse the history to a
// handful of commits.
const CHURN_FRACTION: f64 = 0.1;
const CHURN_CONTENT_LEN: usize = 10;

/// Expected oid of the cached bench repo's aggregate index commit. This is the cache validity key:
/// changing any build parameter above changes a case head, which changes the index commit oid, which
/// then fails the strict check in `provision_repo` and reports the new value to paste here. Filled in
/// by running the bench once after a build change.
const EXPECTED_HEAD: &str = "335b2d17e53df1f9dc051c512c638a647c09e57c";

/// Fixed commit timestamp fed to `josh_actor_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces different
/// head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One history length and the head of its generated history.
struct SizeCase {
    n_commits: usize,
    head: gix_hash::ObjectId,
}

struct SubdirBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark.
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
    // The filter under benchmark: a single `:/<SUBDIR>` subdir selection.
    filter: Filter,
}

impl SubdirBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oids are reproducible and `EXPECTED_HEAD`
        // stays valid across runs. Must run before the cache-miss path invokes the build callback.
        // SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        // Build (or reuse from cache) the bare repo holding every history-length case. On a cache miss
        // the callback builds all cases, tags each tip with a `refs/heads/case_<n_commits>` ref, and
        // returns an aggregate index commit whose oid is the content-addressed cache stamp checked
        // against `EXPECTED_HEAD`.
        let provisioned = josh_test_support::provision_repo::provision_repo(
            "deephistory_subdir",
            &gix_hash::ObjectId::from_str(EXPECTED_HEAD)
                .expect("EXPECTED_HEAD must be a valid oid"),
            |repo| {
                let mut heads = vec![];
                for &n_commits in HISTORY_SIZES {
                    let head = tracing::info_span!(target: "bench", "build_case", n_commits)
                        .in_scope(|| build_case(repo, n_commits))?;
                    heads.push(head);
                }
                build_index(repo, &heads)
            },
        )?;

        // Recover each case head from its ref. This runs identically whether the repo was freshly built
        // or copied from cache.
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

        let filter = Filter::new().subdir(SUBDIR);

        // Pin the sled db open across iterations: per-iteration transaction drops would
        // otherwise close and reopen it, and the cycle's flush/reopen I/O dominates short cases.
        let sled = josh_core::cache::SledCacheBackend::new(provisioned.path());
        sled.pin()?;
        let cache = std::sync::Arc::new(josh_core::cache::CacheStack::new().with_backend(sled));
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // Correctness gate (untimed): confirm the subdir filter produces exactly the SUBDIR subtree of
        // the raw head, so we never silently measure a filter that drops everything or is a no-op. A
        // subdir filter's result tree is the selected directory's content lifted to the root, so the
        // filtered head tree must equal the raw head's `SUBDIR` subtree. Run through a throwaway
        // transaction on the smallest case (the check is history-length independent), then reset caches
        // so nothing here warms the timed runs.
        {
            let transaction = context.open()?;
            let case = cases.first().expect("at least one case");
            let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
            // The gate reads the filtered result through the repository handle, which only
            // sees what is on disk.
            transaction.flush_mem_odb()?;
            let repo = josh_test_support::bench::open_repo(transaction.path())?;
            let filtered_tree = repo.find_commit(filtered)?.tree()?.id();
            let raw_subdir_tree =
                josh_test_support::bench::commit_path(&repo, case.head, Path::new(SUBDIR))?
                    .ok_or_else(|| anyhow::anyhow!("fixture path is missing"))?;
            anyhow::ensure!(
                filtered_tree == raw_subdir_tree,
                "subdir filter did not select `{SUBDIR}` -- benchmark would measure the wrong thing"
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

fn random_string(rng: &mut StdRng, len: usize) -> String {
    (0..len)
        .map(|_| {
            use rand::distr::Alphabetic;
            let ch = Alphabetic.sample(rng) as char;
            ch.to_ascii_lowercase()
        })
        .collect()
}

/// Build a root commit whose tree holds `TREE_FILES` files spread across `N_DIRS` top-level
/// directories, then generate an `n_commits` history that churns ~`CHURN_FRACTION` of the files per
/// commit. The tip is tagged with `refs/heads/case_<n_commits>` so the head is recoverable after the
/// repo round-trips through the cache.
fn build_case(repo: &gix::Repository, n_commits: usize) -> anyhow::Result<gix_hash::ObjectId> {
    use rand::RngExt;

    // Deterministic root tree: file `i` lives at `dir_{i % N_DIRS}/file_{i}`, so files are spread
    // evenly and `SUBDIR` always holds a share of them.

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

    // Tag the tip so `setup` can find this case's head on the cache-hit path, where the build callback
    // never runs. Also keeps the whole history reachable through `git prune`.
    repo.reference(
        format!("refs/heads/case_{n_commits}"),
        head,
        gix::refs::transaction::PreviousValue::Any,
        "bench case tip",
    )?;

    Ok(head)
}

/// Aggregate every case tip under one index commit. Its oid changes whenever any case head changes,
/// making it a faithful content-addressed cache stamp for the entire repo, and it keeps all cases
/// reachable so provision_repo's `git prune` retains the full history. It is never filtered.
fn build_index(
    repo: &gix::Repository,
    heads: &[gix_hash::ObjectId],
) -> anyhow::Result<gix_hash::ObjectId> {
    josh_test_support::bench::build_index(repo, &josh_actor_signature()?, heads)
}

fn deephistory_subdir(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = SubdirBench::setup().expect("set up benchmark");

    let mut group = c.benchmark_group("deephistory_subdir");
    // The longest history costs seconds per iteration, so keep Criterion at its minimum sample count
    // to bound the total wall-clock of a run.
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(case.n_commits as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_commits), |b| {
            b.iter_batched(
                // Per-iteration setup (untimed): start from a cold cache and a fresh transaction so
                // every run does the full filtering work instead of hitting memoized results.
                || {
                    josh_core::reset_caches().expect("reset caches");
                    let transaction = bench.context.open().expect("open transaction");
                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                    (transaction, iter_span)
                },
                // Timed: filter the case head. The setup guards are returned so they are dropped
                // untimed after the measured section.
                |(transaction, iter_span)| {
                    josh_core::filter_commit(&transaction, bench.filter, case.head)
                        .expect("filter commit");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

/// Companion to `deephistory_subdir` that gates the same `:/<SUBDIR>` selection behind a
/// `:rev(<=<head>:...)` cutoff. The added work over the plain subdir bench is exactly the
/// `is_ancestor_of` ancestor walk the rev filter triggers: with the tip at the case head, the
/// walk's ancestor set spans the whole history, and the filtered output stays identical to
/// `deephistory_subdir` -- only the cost differs. This isolates how the ancestor walk reads
/// commits (full `find_commit` vs the parent-only `read_parent_ids`).
fn deephistory_rev(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = SubdirBench::setup().expect("set up benchmark");

    // Correctness gate (untimed): with the tip at the case head every commit is an ancestor, so the
    // rev-gated selection must produce exactly the plain subdir filter's tree. Guards against
    // `is_ancestor_of` silently returning false -- which would apply a Nop and measure the wrong
    // (cheaper) path. Runs on the smallest case, then resets caches so nothing warms the timed runs.
    {
        let case = bench.cases.first().expect("at least one case");
        let rev_filter = Filter::new().rev(vec![(
            RevMatch::AncestorInclusive,
            case.head,
            Filter::new().subdir(SUBDIR),
        )]);
        let transaction = bench.context.open().expect("open transaction");
        let rev_head = josh_core::filter_commit(&transaction, rev_filter, case.head).expect("rev");
        let sub_head =
            josh_core::filter_commit(&transaction, bench.filter, case.head).expect("subdir");
        // Both heads are freshly filtered, so read them through the transaction's objects.
        let tree_of = |oid| {
            josh_core::objects::CommitData::read(transaction.odb(), oid)
                .unwrap()
                .tree_id()
                .unwrap()
        };
        assert_eq!(
            tree_of(rev_head),
            tree_of(sub_head),
            "rev(<=head:/{SUBDIR}) must match the plain subdir filter -- is_ancestor_of misbehaving?"
        );
        drop(transaction);
        josh_core::reset_caches().expect("reset caches");
    }

    let mut group = c.benchmark_group("deephistory_rev");
    group.sample_size(10);
    for case in &bench.cases {
        let rev_filter = Filter::new().rev(vec![(
            RevMatch::AncestorInclusive,
            case.head,
            Filter::new().subdir(SUBDIR),
        )]);
        group.throughput(Throughput::Elements(case.n_commits as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_commits), |b| {
            b.iter_batched(
                || {
                    josh_core::reset_caches().expect("reset caches");
                    let transaction = bench.context.open().expect("open transaction");
                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                    (transaction, iter_span)
                },
                |(transaction, iter_span)| {
                    josh_core::filter_commit(&transaction, rev_filter, case.head)
                        .expect("filter commit");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, deephistory_subdir, deephistory_rev);
criterion_main!(benches);
