use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::Filter;
use josh_core::git::josh_actor_signature;
use josh_core::history::{OrphansMode, unapply_filter};
use josh_test_support::bench::EntryKind;
use rand::prelude::*;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};

// Benches the unapply (push) path of josh: a client pushes commits built on the FILTERED
// history and josh translates them back onto the original history. Two distinct walks
// dominate, and both are exercised here with warm filter caches so the walk itself is
// measured rather than first-time filtering:
//
// - `unapply_extend`: a regular push of K new commits on top of the filtered head. The
//   pushed range is discovered with a `base..tip` walk (RangeWalk); on this linear shape it
//   must stay O(K), independent of history length -- the scaling axis of this bench.
// - `unapply_new_branch`: a push of a branch whose tip is an OLD filtered commit (mid
//   history), with no old filtered oid. Resolving it runs find_new_branch_base ->
//   find_unapply_base, whose lazy discover walk visits ~half the original history checking
//   memoized filter results -- this walk's cost IS the measurement, and it scales with
//   history length.
//
// The repo is the deephistory shape: fixed modest tree, history length as the parameter.
const HISTORY_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[10, 100]
} else {
    &[100, 1_000, 10_000]
};

const TREE_FILES: usize = 200;
const N_DIRS: usize = 10;
const SUBDIR: &str = "dir_00";
const CHURN_FRACTION: f64 = 0.1;
const CHURN_CONTENT_LEN: usize = 10;

// Number of new filtered commits each `unapply_extend` iteration pushes.
const PUSH_LEN: usize = 10;

/// See deephistory_subdir: content-addressed cache stamp of the built repo, reported by
/// provision_repo on mismatch after any build-parameter change. The build is identical to
/// deephistory_subdir's (same parameters, same deterministic generator), so the stamp
/// matches its value.
const EXPECTED_HEAD: &str = "335b2d17e53df1f9dc051c512c638a647c09e57c";

const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

struct SizeCase {
    n_commits: usize,
    head: gix_hash::ObjectId,
    filtered_head: gix_hash::ObjectId,
    /// A filtered commit roughly in the middle of the filtered history (first-parent
    /// chase), the tip of the simulated new-branch push.
    filtered_mid: gix_hash::ObjectId,
}

struct UnapplyBench {
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
    filter: Filter,
}

impl UnapplyBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        let provisioned = josh_test_support::provision_repo::provision_repo(
            "unapply",
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

        let filter = Filter::new().subdir(SUBDIR);

        // Pin the sled db open across iterations: per-iteration transaction drops would
        // otherwise close and reopen it, and the cycle's flush/reopen I/O dominates short cases.
        let sled = josh_core::cache::SledCacheBackend::new(provisioned.path());
        sled.pin()?;
        let cache = std::sync::Arc::new(josh_core::cache::CacheStack::new().with_backend(sled));
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // Warm the filter caches for every case head (untimed): the timed sections measure
        // the unapply walks against memoized filter results, matching a proxy that has
        // already served the filtered repo it now receives a push for.
        let mut cases = vec![];
        {
            let transaction = context.open()?;
            let repo = josh_test_support::bench::open_repo(transaction.path())?;
            for &n_commits in HISTORY_SIZES {
                let head = josh_test_support::bench::ref_target(
                    &repo,
                    &format!("refs/heads/case_{n_commits}"),
                )?;
                let filtered_head = josh_core::filter_commit(&transaction, filter, head)?;
                // The first-parent chase below reads the filtered commits through the
                // repository handle, which only sees what is on disk.
                transaction.flush_mem_odb()?;

                // First-parent chase to the middle of the filtered history.
                let mut filtered_mid = filtered_head;
                for _ in 0..n_commits / 2 {
                    let commit = josh_gix_ext::CommitData::read(&repo.objects, filtered_mid)?;
                    let Some(parent) = commit.parent_ids().next() else {
                        break;
                    };
                    filtered_mid = parent;
                }

                cases.push(SizeCase {
                    n_commits,
                    head,
                    filtered_head,
                    filtered_mid,
                });
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

fn random_string(rng: &mut StdRng, len: usize) -> String {
    (0..len)
        .map(|_| {
            use rand::distr::Alphabetic;
            let ch = Alphabetic.sample(rng) as char;
            ch.to_ascii_lowercase()
        })
        .collect()
}

/// deephistory-style case builder: fixed tree, `n_commits` of ~10% churn, tip tagged as
/// `refs/heads/case_<n_commits>`.
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
    let mut head =
        josh_gix_ext::write_commit(&repo.objects, root_tree, &[], &sig, &sig, "content")?;

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

    repo.reference(
        format!("refs/heads/case_{n_commits}"),
        head,
        gix::refs::transaction::PreviousValue::Any,
        "bench case tip",
    )?;

    Ok(head)
}

fn build_index(
    repo: &gix::Repository,
    heads: &[gix_hash::ObjectId],
) -> anyhow::Result<gix_hash::ObjectId> {
    josh_test_support::bench::build_index(repo, &josh_actor_signature()?, heads)
}

/// Build `PUSH_LEN` commits on top of `base` in FILTERED space, each touching the filtered
/// root's `file_0000` with content salted by `salt` so every bench iteration pushes a chain
/// of commits the caches have never seen.
fn extend_filtered(
    repo: &gix::Repository,
    base: gix_hash::ObjectId,
    salt: usize,
) -> anyhow::Result<gix_hash::ObjectId> {
    let sig = josh_actor_signature()?;
    let mut head = base;

    for i in 0..PUSH_LEN {
        let parent = josh_gix_ext::CommitData::read(&repo.objects, head)?;
        let tree = parent.tree_id()?;
        let mut builder = repo.edit_tree(tree)?;
        let blob = josh_gix_ext::write_blob(&repo.objects, format!("push {salt} {i}").as_bytes())?;
        builder.upsert("file_0000", EntryKind::Blob, blob)?;
        let new_tree = builder.write()?.detach();
        head = josh_gix_ext::write_commit(
            &repo.objects,
            new_tree,
            &[head],
            &sig,
            &sig,
            &format!("push {salt} {i}"),
        )?;
    }
    Ok(head)
}

/// A regular push: K fresh filtered commits on top of the filtered head, unapplied onto the
/// original head. The pushed-range walk must stay O(K) as history grows.
fn unapply_extend(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = UnapplyBench::setup().expect("set up benchmark");
    let salt = AtomicUsize::new(0);

    // Correctness gate (untimed): unapplying a pushed chain and re-filtering the result must
    // reproduce the pushed tip -- otherwise the timed runs unapply garbage.
    {
        let case = bench.cases.first().expect("at least one case");
        let transaction = bench.context.open().expect("open transaction");
        let reference_repo =
            josh_test_support::bench::open_repo(transaction.path()).expect("open repo");
        let tip = extend_filtered(&reference_repo, case.filtered_head, usize::MAX)
            .expect("extend filtered");
        let unapplied = unapply_filter(
            &transaction,
            bench.filter,
            case.head,
            case.filtered_head,
            tip,
            OrphansMode::Fail,
            None,
        )
        .expect("unapply");
        let refiltered =
            josh_core::filter_commit(&transaction, bench.filter, unapplied).expect("refilter");
        assert_eq!(
            refiltered, tip,
            "unapply/apply roundtrip broken -- benchmark would measure the wrong thing"
        );
    }

    let mut group = c.benchmark_group("unapply_extend");
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(PUSH_LEN as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_commits), |b| {
            b.iter_batched(
                // Untimed: a fresh transaction and a never-seen pushed chain. Filter caches
                // for the history stay warm -- only the push delta is new, as in a real push.
                || {
                    let transaction = bench.context.open().expect("open transaction");
                    let reference_repo =
                        josh_test_support::bench::open_repo(transaction.path()).expect("open repo");
                    let tip = extend_filtered(
                        &reference_repo,
                        case.filtered_head,
                        salt.fetch_add(1, Ordering::Relaxed),
                    )
                    .expect("extend filtered");
                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                    (transaction, tip, iter_span)
                },
                |(transaction, tip, iter_span)| {
                    unapply_filter(
                        &transaction,
                        bench.filter,
                        case.head,
                        case.filtered_head,
                        tip,
                        OrphansMode::Fail,
                        None,
                    )
                    .expect("unapply filter");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

/// A new-branch push of a mid-history filtered commit: no old filtered oid, so josh must
/// locate the original commit by walking original history against memoized filter results.
/// The discover walk over ~n/2 commits is the measured cost.
fn unapply_new_branch(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = UnapplyBench::setup().expect("set up benchmark");

    // Correctness gate (untimed): the resolved original must re-filter to the pushed tip.
    {
        let case = bench.cases.first().expect("at least one case");
        let transaction = bench.context.open().expect("open transaction");
        let unapplied = unapply_filter(
            &transaction,
            bench.filter,
            case.head,
            gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            case.filtered_mid,
            OrphansMode::Fail,
            None,
        )
        .expect("unapply");
        let refiltered =
            josh_core::filter_commit(&transaction, bench.filter, unapplied).expect("refilter");
        assert_eq!(
            refiltered, case.filtered_mid,
            "new-branch base resolution broken -- benchmark would measure the wrong thing"
        );
    }

    let mut group = c.benchmark_group("unapply_new_branch");
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(case.n_commits as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_commits), |b| {
            b.iter_batched(
                || {
                    let transaction = bench.context.open().expect("open transaction");
                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                    (transaction, iter_span)
                },
                |(transaction, iter_span)| {
                    unapply_filter(
                        &transaction,
                        bench.filter,
                        case.head,
                        gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                        case.filtered_mid,
                        OrphansMode::Fail,
                        None,
                    )
                    .expect("unapply filter");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, unapply_extend, unapply_new_branch);
criterion_main!(benches);
