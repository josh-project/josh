use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// Tree sizes (number of files) benchmarked. Kept small in debug builds so `--test` runs stay fast.
// The pin parameter is a compose of one `file` filter per pinned path (~10% of the tree), rebuilt for
// every commit, so cost grows with both the pinned count and the number of commits.
const SIZES: &[usize] = if cfg!(debug_assertions) {
    &[50, 500]
} else {
    &[100, 1_000, 10_000]
};

// Number of history commits generated on top of the initial commit. The pinned set evolves per
// commit, so the pin parameter is rebuilt and re-applied at each of these.
const N_COMMITS: usize = if cfg!(debug_assertions) { 5 } else { 10 };

// Fraction of the tree's files that each commit changes and then pins. Pinning exactly the churned
// set keeps the parameter at ~size/10 `file` filters (rebuilt fresh at every commit) and guarantees
// every pinned path actually changed, so the hold has a visible effect (see the sanity gate).
const PIN_FRACTION: f64 = 0.1;
const CHURN_CONTENT_LEN: usize = 10;

const PATH_COMPONENT_LENGTH: usize = 15;
const NESTING_LEVEL: usize = 3;
const N_PER_SUBFOLDER_MIN: usize = 10;
const N_PER_SUBFOLDER_MAX: usize = 100;

// The single hook argument. The outer filter is `:hook=pin`, so `arg` in `filter_for_commit` is
// always this string.
const HOOK_ARG: &str = "pin";

// Why a hook (and not a plain `:pin` filter)? `:pin`'s hold-back logic lives in `per_rev_filter`,
// which josh only invokes for the per-revision filter sources -- `:workspace`, `:+stored`, and
// `:hook`. A bare `Filter::new().pin(..)` fed straight to `filter_commit` is a tree-level no-op
// (`Op::Pin(_) => Ok(x)`), so it would measure nothing. The original `ultrawide_pin` bench drives
// pin through a stored `workspace.josh`, which pays a filter parse+legalize per commit; the hook
// serves a pre-built per-commit filter by oid instead, isolating the pin evaluation itself.
struct BenchPinHook {
    per_commit: HashMap<git2::Oid, josh_filter::Filter>,
}

impl josh_core::cache::FilterHook for BenchPinHook {
    fn filter_for_commit(
        &self,
        commit_oid: git2::Oid,
        arg: &str,
    ) -> anyhow::Result<josh_filter::Filter> {
        anyhow::ensure!(arg == HOOK_ARG, "unexpected pin hook arg: {arg}");
        self.per_commit
            .get(&commit_oid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("no pin filter for commit {commit_oid}"))
    }
}

/// One tree size and the head of its generated history. The per-commit pin filters live in the
/// shared hook.
struct SizeCase {
    size: usize,
    head: git2::Oid,
}

struct PinBench {
    // Keeps the on-disk repository alive for the duration of the benchmark.
    _tmp: tempfile::TempDir,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
    // Attached to every opened transaction so `:hook=pin` resolves per commit.
    hook: Arc<BenchPinHook>,
    // The outer filter under benchmark: `:~(history="no-splice")[:hook=pin]`.
    filter: josh_filter::Filter,
}

impl PinBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        let tmp = tempfile::tempdir()?;
        let repo = git2::Repository::init_bare(tmp.path())?;

        let mut per_commit = HashMap::new();

        let cases = SIZES
            .iter()
            .map(|&size| {
                tracing::info_span!(target: "bench", "build_case", size)
                    .in_scope(|| build_case(&repo, size, &mut per_commit))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let hook = Arc::new(BenchPinHook { per_commit });
        let filter = josh_filter::Filter::new()
            .hook(HOOK_ARG)
            .with_meta("history", "no-splice");

        josh_core::cache::sled_load(tmp.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );
        let context = josh_core::cache::TransactionContext::new(tmp.path(), cache);

        // Sanity gate (untimed): confirm the filter actually holds paths back, so we never silently
        // measure a no-op pin. Because the pinned set churns, the filtered head tree must differ from
        // the raw head tree. Run through a throwaway transaction, then reset caches so nothing here
        // warms the timed runs.
        {
            let transaction = context.open()?.with_filter_hook(hook.clone());
            for case in &cases {
                let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
                let filtered_tree = transaction.repo().find_commit(filtered)?.tree()?.id();
                let raw_tree = transaction.repo().find_commit(case.head)?.tree()?.id();
                anyhow::ensure!(
                    filtered_tree != raw_tree,
                    "pin had no visible effect for size {} -- benchmark would measure a no-op",
                    case.size
                );
            }
        }
        josh_core::reset_caches()?;

        Ok(Self {
            _tmp: tmp,
            context,
            cases,
            hook,
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

/// The per-commit pin filter: pin the identity tree by a compose of one `file` filter per pinned
/// path (an empty pin parameter when nothing is pinned).
fn pin_filter(pinned: &[PathBuf]) -> josh_filter::Filter {
    let param = if pinned.is_empty() {
        josh_filter::Filter::new().empty()
    } else {
        let files = pinned
            .iter()
            .map(|path| josh_filter::Filter::new().file(path))
            .collect::<Vec<_>>();
        josh_filter::compose(&files)
    };
    josh_filter::Filter::new().pin(param)
}

/// Build a commit whose tree holds `size` files, then generate an N_COMMITS history that churns
/// files and evolves the pinned set per commit. Each commit's pin filter is recorded in `per_commit`
/// keyed by commit oid so the hook can serve it.
fn build_case(
    repo: &git2::Repository,
    size: usize,
    per_commit: &mut HashMap<git2::Oid, josh_filter::Filter>,
) -> anyhow::Result<SizeCase> {
    use rand::RngExt;

    // Distribute files uniformly across nested subfolders.
    let mut rng = StdRng::seed_from_u64(0);
    let files_in_folder =
        rand::distr::Uniform::try_from(N_PER_SUBFOLDER_MIN..=N_PER_SUBFOLDER_MAX)?;

    let mut builder = git2::build::TreeUpdateBuilder::new();
    let mut all_paths = vec![];

    while all_paths.len() < size {
        let mut subpath = PathBuf::new();
        for _ in 0..NESTING_LEVEL {
            subpath.push(random_string(&mut rng, PATH_COMPONENT_LENGTH));
        }

        let to_add = files_in_folder.sample(&mut rng).min(size - all_paths.len());
        for i in 0..to_add {
            let file_name = format!("file_{}", i);
            let full_path = subpath.join(&file_name);
            let oid = repo.blob(file_name.as_bytes())?;
            builder.upsert(&full_path, oid, git2::FileMode::Blob);
            all_paths.push(full_path);
        }
    }

    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let root_tree = repo.find_tree(builder.create_updated(repo, &baseline)?)?;

    let sig = josh_commit_signature()?;
    // No ref update -- the commit oid is tracked directly per case.
    let mut head = repo.commit(None, &sig, &sig, "content", &root_tree, &[])?;

    // The root commit has no parent, so `:pin` has nothing to hold; record an empty-set filter so
    // the hook can resolve every commit in the history.
    per_commit.insert(head, pin_filter(&[]));

    // Deterministic evolving history: each commit changes a fresh random ~PIN_FRACTION of the files
    // and pins exactly those. The pinned set differs at every commit (forcing the parameter to be
    // rebuilt each time) and always holds paths that just changed, so the filtered head genuinely
    // differs from the raw head.
    let mut rng = StdRng::seed_from_u64(1);
    for i in 0..N_COMMITS {
        let parent = repo.find_commit(head)?;
        let tree = parent.tree()?;
        let mut builder = git2::build::TreeUpdateBuilder::new();

        let pinned = all_paths
            .iter()
            .filter(|_| rng.random_bool(PIN_FRACTION))
            .cloned()
            .collect::<Vec<_>>();

        for path in &pinned {
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

        per_commit.insert(head, pin_filter(&pinned));
    }

    Ok(SizeCase { size, head })
}

fn ultrawide_pin_hook(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = PinBench::setup().expect("set up benchmark");

    let mut group = c.benchmark_group("ultrawide_pin_hook");
    // The largest tree costs seconds per iteration, so keep Criterion at its minimum sample count to
    // bound the total wall-clock of a run.
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(case.size as u64));

        group.bench_function(BenchmarkId::new("per_path", case.size), |b| {
            b.iter_with_setup_wrapper(|runner| {
                // Per-iteration setup (untimed): start from a cold cache and a fresh transaction so
                // every run does the full filtering work instead of hitting memoized results. The
                // hook is re-attached because it lives on the transaction, not the context.
                josh_core::reset_caches().expect("reset caches");
                let transaction = bench
                    .context
                    .open()
                    .expect("open transaction")
                    .with_filter_hook(bench.hook.clone());

                // Optional: route object writes through an in-memory mempack backend to measure the
                // ODB write-path overhead. The odb is bound first so it outlives the mempack.
                let mempack_odb = std::env::var_os("JOSH_BENCH_MEMPACK")
                    .map(|_| transaction.repo().odb().expect("odb"));
                let _mempack = mempack_odb.as_ref().map(|odb| {
                    odb.add_new_mempack_backend(1000)
                        .expect("add mempack backend")
                });

                let iter_span = tracing::info_span!(target: "bench", "iter").entered();

                runner.run(|| {
                    josh_core::filter_commit(&transaction, bench.filter, case.head)
                        .expect("filter commit")
                });

                drop(iter_span);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, ultrawide_pin_hook);
criterion_main!(benches);
