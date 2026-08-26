use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::Filter;
use josh_core::git::josh_actor_signature;
use josh_test_support::bench::{EntryKind, build_index, random_string};
use rand::prelude::*;
use std::path::{Path, PathBuf};
use std::str::FromStr;

// This bench measures the *reference* surface: enumerating refs by prefix, resolving each to an oid,
// looking up the (cache-hot) filtered result per ref, and force-writing the filtered refs back. The
// scaling parameter is the number of refs, and the filter caches are deliberately warmed during
// setup and never reset, so the measured loop is dominated by ref enumeration, resolution and
// updates rather than by filtering work -- the proxy's steady state when serving a repo whose
// filter results are already cached. The filtering benches cover the cold path; this one exists so
// the ref backend (enumeration, loose/packed lookup, ref writes) has its own regression gate.

// Number of change refs (each pointing to a distinct commit of a churned history) per case. Kept
// small in debug builds so `cargo test`/`--test` runs stay fast.
const REF_COUNTS: &[usize] = if cfg!(debug_assertions) {
    &[10]
} else {
    &[100, 1_000]
};

// A fixed, modest tree shared as the root of every case, same shape as `deephistory_subdir`.
const TREE_FILES: usize = 200;
const N_DIRS: usize = 10;

// The directory the benchmarked filter selects; churn keeps touching it so per-ref filter results
// stay distinct and non-trivial.
const SUBDIR: &str = "dir_00";

// Fraction of the tree's files each commit changes ("churns").
const CHURN_FRACTION: f64 = 0.1;
const CHURN_CONTENT_LEN: usize = 10;

/// Expected oid of the cached bench repo's aggregate index commit. This is the cache validity key:
/// changing any build parameter above changes a case head, which changes the index commit oid,
/// which then fails the strict check in `provision_repo` and reports the new value to paste here.
/// Filled in by running the bench once after a build change. Debug builds use reduced REF_COUNTS,
/// so they have their own expected oid and their own provision-cache name (below) -- otherwise
/// `cargo test --benches` and `cargo bench` would fight over the same cache entry.
const EXPECTED_HEAD: &str = if cfg!(debug_assertions) {
    "63a90b791bb6ac771f445f4f085001eae6362a1e"
} else {
    "85136c91e53cddde412086e85d25a33d05931ff0"
};

/// Provision-cache name, split per profile to match the per-profile EXPECTED_HEAD.
const CACHE_NAME: &str = if cfg!(debug_assertions) {
    "refs_filter_update_debug"
} else {
    "refs_filter_update"
};

/// Fixed commit timestamp fed to `josh_actor_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces
/// different head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One ref-count case and the head of its generated history.
struct SizeCase {
    n_refs: usize,
    head: gix_hash::ObjectId,
}

struct RefsBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark.
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
    // The filter under benchmark: a single `:/<SUBDIR>` subdir selection.
    filter: Filter,
}

impl RefsBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oids are reproducible and
        // `EXPECTED_HEAD` stays valid across runs. Must run before the cache-miss path invokes the
        // build callback. SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        // Build (or reuse from cache) the bare repo holding every case. On a cache miss the
        // callback builds all cases -- a churned linear history per case, one
        // `refs/heads/case_<n>/change_<i>` ref per commit plus a `refs/heads/case_<n>_tip` ref --
        // and returns an aggregate index commit whose oid is the content-addressed cache stamp
        // checked against `EXPECTED_HEAD`.
        let provisioned = josh_test_support::provision_repo::provision_repo(
            CACHE_NAME,
            &gix_hash::ObjectId::from_str(EXPECTED_HEAD)
                .expect("EXPECTED_HEAD must be a valid oid"),
            |repo| {
                let mut heads = vec![];
                for &n_refs in REF_COUNTS {
                    let head = tracing::info_span!(target: "bench", "build_case", n_refs)
                        .in_scope(|| build_case(repo, n_refs))?;
                    heads.push(head);
                }
                build_index(repo, &josh_actor_signature()?, &heads)
            },
        )?;

        // Recover each case head from its tip ref. This runs identically whether the repo was
        // freshly built or copied from cache.
        let mut cases = vec![];
        {
            let repo = &provisioned.repo;
            for &n_refs in REF_COUNTS {
                let head = josh_test_support::bench::ref_target(
                    repo,
                    &format!("refs/heads/case_{n_refs}_tip"),
                )?;
                cases.push(SizeCase { n_refs, head: head });
            }
        }

        let filter = Filter::new().subdir(SUBDIR);

        // Pin the sled db open across iterations: per-iteration transaction drops would
        // otherwise close and reopen it, and the cycle's flush/reopen I/O dominates short cases.
        let sled = josh_core::cache::SledCacheBackend::new(provisioned.path());
        sled.pin()?;
        let cache = std::sync::Arc::new(josh_core::cache::CacheStack::new().with_backend(sled));
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // Correctness gate (untimed): confirm the subdir filter selects exactly the SUBDIR subtree
        // of a case head, so we never silently measure a filter that drops everything or is a
        // no-op.
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

        // Warm the filter caches for every case: filtering each head walks and memoizes its whole
        // ancestry, so every per-ref lookup in the timed loop is a cache hit. Deliberately NOT
        // followed by `reset_caches` -- warm caches are the point (see module comment).
        {
            let transaction = context.open()?;
            for case in &cases {
                josh_core::filter_commit(&transaction, filter, case.head)?;
            }
        }

        Ok(Self {
            _repo: provisioned,
            context,
            cases,
            filter,
        })
    }
}

/// Build a root commit whose tree holds `TREE_FILES` files spread across `N_DIRS` top-level
/// directories, then generate `n_refs` churn commits, pointing `refs/heads/case_<n>/change_<i>` at
/// each. The tip additionally gets `refs/heads/case_<n>_tip` so the head is recoverable after
/// the repo round-trips through the cache.
fn build_case(repo: &gix::Repository, n_refs: usize) -> anyhow::Result<gix_hash::ObjectId> {
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
    let mut head =
        josh_gix_ext::write_commit(&repo.objects, root_tree, &[], &sig, &sig, "content")?;

    // Deterministic history: each commit churns a fresh random ~CHURN_FRACTION of the files and
    // gets its own change ref, so every ref resolves to a distinct commit.
    let mut rng = StdRng::seed_from_u64(1);
    for i in 0..n_refs {
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
        repo.reference(
            format!("refs/heads/case_{n_refs}/change_{i:04}"),
            head,
            gix::refs::transaction::PreviousValue::Any,
            "bench change ref",
        )?;
    }

    // Tip ref so `setup` can find this case's head on the cache-hit path, where the build callback
    // never runs. Named `_tip` (not `case_<n>`) because `refs/heads/case_<n>/change_<i>` makes
    // `case_<n>` a ref *directory*, and a ref cannot shadow a directory.
    repo.reference(
        format!("refs/heads/case_{n_refs}_tip"),
        head,
        gix::refs::transaction::PreviousValue::Any,
        "bench case tip",
    )?;

    Ok(head)
}

fn refs_filter_update(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = RefsBench::setup().expect("set up benchmark");

    let mut group = c.benchmark_group("refs_filter_update");
    group.sample_size(10);
    for case in &bench.cases {
        let ref_prefix = format!("refs/heads/case_{}/change_", case.n_refs);
        group.throughput(Throughput::Elements(case.n_refs as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_refs), |b| {
            b.iter_batched(
                // Per-iteration setup (untimed): a fresh transaction only. Caches stay warm across
                // iterations by design (see module comment).
                || {
                    let transaction = bench.context.open().expect("open transaction");
                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                    (transaction, iter_span)
                },
                // Timed: enumerate the change refs, resolve each to its oid, look up the filtered
                // commit per ref (cache-hot), and force-write every filtered ref under
                // `refs/josh/bench/`. This is the proxy's fetch steady state.
                |(transaction, iter_span)| {
                    let mut refs = vec![];
                    transaction
                        .for_each_ref_prefixed(&ref_prefix, |name, oid| {
                            refs.push((name.to_string(), oid));
                            Ok(())
                        })
                        .expect("iterate refs");
                    assert_eq!(
                        refs.len(),
                        case.n_refs,
                        "ref enumeration must see every ref"
                    );

                    let (updated, errors) =
                        josh_core::filter_refs(&transaction, bench.filter, &refs);
                    assert!(errors.is_empty(), "no ref may fail to filter");

                    let updated = updated
                        .into_iter()
                        .map(|(name, oid)| {
                            let renamed = name.replacen("refs/heads/", "refs/josh/bench/", 1);
                            (renamed, oid)
                        })
                        .collect::<Vec<_>>();
                    josh_core::update_refs(&transaction, updated);

                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, refs_filter_update);
criterion_main!(benches);
