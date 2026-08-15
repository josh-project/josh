use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::Filter;
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::path::{Path, PathBuf};

// The distributed-cache counterpart to `deephistory_subdir_sparse`: the same long, sparse-output
// history and `:/dir_00` filter, but with a `DistributedCacheBackend` added to the cache stack. It
// measures the backend's per-commit overhead when filtering a long history through a cold
// distributed cache.

// Number of history commits generated on top of the root commit for each case. Kept small in debug
// builds so `cargo test`/`--test` runs stay fast.
const HISTORY_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[10, 100]
} else {
    &[100, 1_000, 10_000]
};

// A fixed, modest tree: TREE_FILES files spread evenly across N_DIRS top-level directories
// (`dir_00`..`dir_{N_DIRS-1}`). `dir_00` -- the directory the filter selects -- holds TREE_FILES /
// N_DIRS of them.
const TREE_FILES: usize = 200;
const N_DIRS: usize = 10;
const SUBDIR: &str = "dir_00";

// Every commit churns exactly CHURN_PER_COMMIT files drawn from `dir_01`..`dir_{N_DIRS-1}` (never
// `dir_00`), so every commit is a real input commit that the `:/dir_00` filter maps to no change...
const CHURN_PER_COMMIT: usize = 20;
const CHURN_CONTENT_LEN: usize = 10;
// ...except with probability SUBDIR_CHANGE_PROB, where it additionally touches one `dir_00` file and
// therefore produces an output commit. At 0.02 a 10k-commit history yields ~180 output commits, an
// input:output ratio near 55:1.
const SUBDIR_CHANGE_PROB: f64 = 0.02;

/// Expected oid of the cached bench repo's aggregate index commit -- the cache validity key. The
/// build parameters above are identical to `deephistory_subdir_sparse`, so this is the same oid.
/// Changing any build parameter changes it and fails the strict check in `provision_repo`, which
/// reports the new value to paste here.
const EXPECTED_HEAD: &str = "cb9ccbca757fe9707010dde47bed3eeba094679e";

/// Fixed commit timestamp fed to `josh_commit_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible and `EXPECTED_HEAD` stays stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One history length and the head of its generated history.
struct SizeCase {
    n_commits: usize,
    head: git2::Oid,
}

struct SubdirBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark.
    repo: josh_test_support::provision_repo::ProvisionedRepo,
    cases: Vec<SizeCase>,
    // The filter under benchmark: a single `:/<SUBDIR>` subdir selection.
    filter: Filter,
}

impl SubdirBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oids are reproducible. Must run before the
        // cache-miss path invokes the build callback.
        // SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        let provisioned = josh_test_support::provision_repo::provision_repo(
            "deephistory_subdir_distributed",
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

        // Recover each case head from its ref. Runs identically whether the repo was freshly built or
        // copied from cache.
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

        // Correctness + shape gate (untimed): confirm the subdir filter produces exactly the SUBDIR
        // subtree of the raw head, and that the filtered history is far shorter than the input history
        // -- otherwise this bench would stop measuring the input >> output regime it exists for.
        // Checked on the largest case with a throwaway cache; caches are reset afterwards so nothing
        // here warms the timed runs.
        {
            let cache = std::sync::Arc::new(
                josh_core::cache::CacheStack::new()
                    .with_backend(josh_core::cache::SledCacheBackend::default()),
            );
            let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);
            let transaction = context.open()?;
            let case = cases.last().expect("at least one case");
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
            let input = case.n_commits + 1;
            let output = count_history(repo, filtered)?;
            anyhow::ensure!(
                output * 10 < input,
                "expected input >> output, got {input} input / {output} output commits"
            );
            tracing::info!(
                target: "bench",
                "sparse subdir: {input} input commits -> {output} output commits ({:.1}x)",
                input as f64 / output as f64
            );
        }
        josh_core::reset_caches()?;
        clear_cache_refs(&provisioned.repo)?;

        Ok(Self {
            repo: provisioned,
            cases,
            filter,
        })
    }
}

/// Number of commits reachable from `head`.
fn count_history(repo: &git2::Repository, head: git2::Oid) -> anyhow::Result<usize> {
    let mut walk = repo.revwalk()?;
    walk.push(head)?;
    Ok(walk.count())
}

/// Delete every `refs/josh/cache/*` ref the distributed backend may have written, so each timed
/// iteration starts from a genuinely cold distributed cache and not from refs a previous iteration
/// flushed on drop.
fn clear_cache_refs(repo: &git2::Repository) -> anyhow::Result<()> {
    for r in repo.references_glob("refs/josh/cache/*")? {
        r?.delete()?;
    }
    Ok(())
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

/// Build a root commit whose tree holds `TREE_FILES` files across `N_DIRS` directories, then generate
/// an `n_commits` history whose churn almost always avoids `dir_00`: every commit changes
/// `CHURN_PER_COMMIT` files outside it, and only with probability `SUBDIR_CHANGE_PROB` also touches a
/// `dir_00` file. The tip is tagged `refs/heads/case_<n_commits>` so the head survives the cache
/// round-trip.
fn build_case(repo: &git2::Repository, n_commits: usize) -> anyhow::Result<git2::Oid> {
    use rand::RngExt;

    // Deterministic root tree: file `i` lives at `dir_{i % N_DIRS}/file_{i}`; split the paths into the
    // filtered subdir (`dir_00`) and the rest so churn can target each set independently.
    let mut builder = git2::build::TreeUpdateBuilder::new();
    let mut subdir_paths = vec![];
    let mut other_paths = vec![];
    for i in 0..TREE_FILES {
        let dir = i % N_DIRS;
        let path = PathBuf::from(format!("dir_{dir:02}")).join(format!("file_{i:04}"));
        let oid = repo.blob(path.to_string_lossy().as_bytes())?;
        builder.upsert(&path, oid, git2::FileMode::Blob);
        if dir == 0 {
            subdir_paths.push(path);
        } else {
            other_paths.push(path);
        }
    }

    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let root_tree = repo.find_tree(builder.create_updated(repo, &baseline)?)?;

    let sig = josh_commit_signature()?;
    // No ref update yet -- the tip ref is set once the history is complete.
    let mut head = repo.commit(None, &sig, &sig, "content", &root_tree, &[])?;

    let mut rng = StdRng::seed_from_u64(1);
    for i in 0..n_commits {
        let parent = repo.find_commit(head)?;
        let tree = parent.tree()?;
        let mut builder = git2::build::TreeUpdateBuilder::new();

        // Always churn non-dir_00 files (guarantees a real input commit, no effect on the filter).
        // Dedup indices so a path is never upserted twice in the same commit.
        let mut idxs = std::collections::BTreeSet::new();
        while idxs.len() < CHURN_PER_COMMIT {
            idxs.insert(rng.random_range(0..other_paths.len()));
        }
        for idx in idxs {
            let path = &other_paths[idx];
            let blob = repo.blob(random_string(&mut rng, CHURN_CONTENT_LEN).as_bytes())?;
            builder.upsert(path, blob, git2::FileMode::Blob);
        }
        // Rarely also touch dir_00, producing an output commit.
        if rng.random_bool(SUBDIR_CHANGE_PROB) {
            let path = &subdir_paths[rng.random_range(0..subdir_paths.len())];
            let blob = repo.blob(random_string(&mut rng, CHURN_CONTENT_LEN).as_bytes())?;
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

    repo.reference(
        &format!("refs/heads/case_{n_commits}"),
        head,
        true,
        "bench case tip",
    )?;
    Ok(head)
}

/// Aggregate every case tip under one index commit so its oid is a content-addressed cache stamp for
/// the whole repo and every case stays reachable through provision_repo's `git prune`. Never filtered.
fn build_index(repo: &git2::Repository, heads: &[git2::Oid]) -> anyhow::Result<git2::Oid> {
    let sig = josh_commit_signature()?;
    let empty_tree = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let parents = heads
        .iter()
        .map(|oid| repo.find_commit(*oid))
        .collect::<Result<Vec<_>, _>>()?;
    let parent_refs = parents.iter().collect::<Vec<_>>();
    Ok(repo.commit(
        Some("refs/heads/bench-index"),
        &sig,
        &sig,
        "bench index",
        &empty_tree,
        &parent_refs,
    )?)
}

fn deephistory_subdir_distributed(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=info");

    let bench = SubdirBench::setup().expect("set up benchmark");
    let path = bench.repo.path().to_path_buf();

    let mut group = c.benchmark_group("deephistory_subdir_distributed");
    // The longest history costs ~100 ms per iteration, so keep Criterion at its minimum sample count.
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(case.n_commits as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_commits), |b| {
            b.iter_batched(
                // Per-iteration setup (untimed): cold caches and a fresh transaction so every run does
                // the full filtering work through a cold distributed cache. The cache stack mirrors the
                // `josh` CLI default: a sled backend plus a distributed backend. The
                // distributed backend is opened writable (the `josh cache build` mode) so the
                // bench keeps covering the write/buffering path; the read-only session default
                // only does less.
                || {
                    josh_core::reset_caches().expect("reset caches");
                    clear_cache_refs(&bench.repo.repo).expect("clear cache refs");
                    let cache = std::sync::Arc::new(
                        josh_core::cache::CacheStack::new()
                            .with_backend(josh_core::cache::SledCacheBackend::default())
                            .with_backend(
                                josh_core::cache::DistributedCacheBackend::writable(&path)
                                    .expect("open distributed cache"),
                            ),
                    );
                    josh_core::cache::TransactionContext::new(&path, cache)
                        .open()
                        .expect("open transaction")
                },
                // Timed: filter the case head. The transaction is returned so it is dropped untimed
                // after the measured section.
                |transaction| {
                    josh_core::filter_commit(&transaction, bench.filter, case.head)
                        .expect("filter commit");
                    transaction
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, deephistory_subdir_distributed);
criterion_main!(benches);
