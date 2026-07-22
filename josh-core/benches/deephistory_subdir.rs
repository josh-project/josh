use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::{Filter, RevMatch};
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::path::{Path, PathBuf};

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

/// Fixed commit timestamp fed to `josh_commit_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces different
/// head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One history length and the head of its generated history.
struct SizeCase {
    n_commits: usize,
    head: git2::Oid,
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
            &git2::Oid::from_str(EXPECTED_HEAD).expect("EXPECTED_HEAD must be a valid oid"),
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
                let head = repo.refname_to_id(&format!("refs/heads/case_{n_commits}"))?;
                cases.push(SizeCase { n_commits, head });
            }
        }

        let filter = Filter::new().subdir(SUBDIR);

        josh_core::cache::sled_load(provisioned.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );
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
            let repo = transaction.repo();
            let filtered_tree = repo.find_commit(filtered)?.tree()?.id();
            let raw_subdir_tree = repo
                .find_commit(case.head)?
                .tree()?
                .get_path(Path::new(SUBDIR))?
                .id();
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
fn build_case(repo: &git2::Repository, n_commits: usize) -> anyhow::Result<git2::Oid> {
    use rand::RngExt;

    // Deterministic root tree: file `i` lives at `dir_{i % N_DIRS}/file_{i}`, so files are spread
    // evenly and `SUBDIR` always holds a share of them.
    let mut builder = git2::build::TreeUpdateBuilder::new();
    let mut all_paths = vec![];
    for i in 0..TREE_FILES {
        let path = PathBuf::from(format!("dir_{:02}", i % N_DIRS)).join(format!("file_{i:04}"));
        let oid = repo.blob(path.to_string_lossy().as_bytes())?;
        builder.upsert(&path, oid, git2::FileMode::Blob);
        all_paths.push(path);
    }

    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let root_tree = repo.find_tree(builder.create_updated(repo, &baseline)?)?;

    let sig = josh_commit_signature()?;
    // No ref update yet -- the tip ref is set once the history is complete.
    let mut head = repo.commit(None, &sig, &sig, "content", &root_tree, &[])?;

    // Deterministic history: each commit churns a fresh random ~CHURN_FRACTION of the files.
    let mut rng = StdRng::seed_from_u64(1);
    for i in 0..n_commits {
        let parent = repo.find_commit(head)?;
        let tree = parent.tree()?;
        let mut builder = git2::build::TreeUpdateBuilder::new();

        let churned = all_paths
            .iter()
            .filter(|_| rng.random_bool(CHURN_FRACTION))
            .cloned()
            .collect::<Vec<_>>();

        for path in &churned {
            let content = random_string(&mut rng, CHURN_CONTENT_LEN);
            let blob = repo.blob(content.as_bytes())?;
            builder.upsert(path, blob, git2::FileMode::Blob);
        }

        let new_tree = repo.find_tree(builder.create_updated(repo, &tree)?)?;
        head = repo.commit(
            None,
            &sig,
            &sig,
            &format!("commit {i}"),
            &new_tree,
            &[&parent],
        )?;
    }

    // Tag the tip so `setup` can find this case's head on the cache-hit path, where the build callback
    // never runs. Also keeps the whole history reachable through `git prune`.
    repo.reference(
        &format!("refs/heads/case_{n_commits}"),
        head,
        true,
        "bench case tip",
    )?;

    Ok(head)
}

/// Aggregate every case tip under one index commit. Its oid changes whenever any case head changes,
/// making it a faithful content-addressed cache stamp for the entire repo, and it keeps all cases
/// reachable so provision_repo's `git prune` retains the full history. It is never filtered.
fn build_index(repo: &git2::Repository, heads: &[git2::Oid]) -> anyhow::Result<git2::Oid> {
    let sig = josh_commit_signature()?;
    let empty_tree = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let parents = heads
        .iter()
        .map(|oid| repo.find_commit(*oid))
        .collect::<Result<Vec<_>, _>>()?;
    let parent_refs = parents.iter().collect::<Vec<_>>();
    let index = repo.commit(
        Some("refs/heads/bench-index"),
        &sig,
        &sig,
        "bench index",
        &empty_tree,
        &parent_refs,
    )?;
    Ok(index)
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
/// `is_ancestor_of` ancestor walk the rev filter triggers: `get_rev_filter` asks, per commit,
/// whether it is an ancestor of the tip, which builds (once per `filter_commit`, since the
/// `ANCESTORS` set is memoized and `reset_caches` clears it each iteration) the full ancestor set
/// of the tip. The tip here is the case head, so its ancestor set is the whole filtered history --
/// the case where that walk overlaps `walk2`'s own commit reads. Since every commit is `<= head`,
/// the filtered output is identical to `deephistory_subdir`; only the cost differs. This isolates
/// how the ancestor walk reads commits (full `find_commit` vs the parent-only `read_parent_ids`).
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
        let tree_of = |oid| transaction.repo().find_commit(oid).unwrap().tree_id();
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
        // `:rev(<=<case head>:/<SUBDIR>)` -- every commit in the history is an ancestor of the case
        // head, so all match and receive the subdir selection.
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
