use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::path::PathBuf;

// Tree sizes (number of files) where *both* approaches are benchmarked. Kept small in debug builds
// so `--test` runs stay fast. Capped at 10k because each case now filters a whole N_COMMITS history
// (see below), which multiplies the per-blob cost roughly by the number of commits.
const SIZES: &[usize] = if cfg!(debug_assertions) {
    &[50, 500]
} else {
    &[100, 1_000, 10_000]
};

// Larger tree sizes where only the one-tree approach is benchmarked: at this scale the per-blob
// chain is impractically slow, whereas the single tree insert stays cheap, so it is the only one
// worth measuring here.
const ONE_TREE_ONLY_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[5_000]
} else {
    &[500_000]
};

// Number of history commits generated on top of the initial commit; the benchmark filters the head
// of this history, so filter_commit walks all of them (like `ultrawide_pin`).
const N_COMMITS: usize = if cfg!(debug_assertions) { 5 } else { 10 };

// Fraction of the tree's files that get replaced with a fixed blob.
const REPLACE_FRACTION: f64 = 0.1;

// Per commit, each replaced ("pinned") file is re-randomized upstream with this probability, so the
// history keeps changing exactly the files the filter holds to a fixed blob. The non-replaced 90%
// stays constant across the whole history, which is what keeps the two approaches equivalent: the
// one-tree filter inserts a pre-built result subtree, and that subtree is only correct for every
// commit if the files it does not overwrite never change.
const PROB_CHURN: f64 = 0.5;
const CHURN_CONTENT_LEN: usize = 10;

const PATH_COMPONENT_LENGTH: usize = 15;
const NESTING_LEVEL: usize = 3;
const N_PER_SUBFOLDER_MIN: usize = 10;
const N_PER_SUBFOLDER_MAX: usize = 100;

// All generated files live under this single top-level directory so the "one tree" approach can
// replace the entire content with a *single* Op::Insert of one tree oid. (Op::Insert inverts to an
// exclude of the whole destination path, so inserting a subtree at `content` replaces all of
// `content/`; hence the inserted subtree must contain every file.)
const CONTENT_DIR: &str = "content";

/// One tree size and the two filters under comparison, both of which replace ~10% of the files with
/// a fixed blob while keeping the rest. `head` is the tip of a generated N_COMMITS history.
struct SizeCase {
    size: usize,
    head: git2::Oid,
    // Approach A: one Op::Insert per replaced file (blob oid), all composed together. Filter-engine
    // work scales with the number of replaced files. `None` for one-tree-only sizes.
    filter_per_blob: Option<josh_filter::Filter>,
    // Approach B: a single Op::Insert of one pre-built tree (tree oid) containing the whole result.
    // Near-constant filter-engine work regardless of how many files are replaced.
    filter_one_tree: josh_filter::Filter,
}

struct InsertBench {
    // Keeps the on-disk repository alive for the duration of the benchmark.
    _tmp: tempfile::TempDir,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
}

impl InsertBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        let tmp = tempfile::tempdir()?;
        let repo = git2::Repository::init_bare(tmp.path())?;

        let cases = SIZES
            .iter()
            .map(|&size| (size, true))
            .chain(ONE_TREE_ONLY_SIZES.iter().map(|&size| (size, false)))
            .map(|(size, with_per_blob)| {
                tracing::info_span!(target: "bench", "build_case", size)
                    .in_scope(|| build_case(&repo, size, with_per_blob))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        josh_core::cache::sled_load(tmp.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );
        let context = josh_core::cache::TransactionContext::new(tmp.path(), cache);

        // Correctness gate (untimed): where both approaches exist they must produce the same result
        // tree. Run through a throwaway transaction, then reset caches so nothing here warms the
        // timed runs. One-tree-only sizes have nothing to compare against and are skipped.
        {
            let transaction = context.open()?;
            for case in &cases {
                let Some(filter_per_blob) = case.filter_per_blob else {
                    continue;
                };
                let a = filtered_tree(&transaction, filter_per_blob, case.head)?;
                let b = filtered_tree(&transaction, case.filter_one_tree, case.head)?;
                anyhow::ensure!(
                    a == b,
                    "per-blob and one-tree approaches disagree for size {}",
                    case.size
                );
            }
        }
        josh_core::reset_caches()?;

        Ok(Self {
            _tmp: tmp,
            context,
            cases,
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

/// Filtered tree oid of `filter` applied to `head` -- used only for the untimed correctness check.
fn filtered_tree(
    transaction: &josh_core::cache::Transaction,
    filter: josh_filter::Filter,
    head: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let filtered = josh_core::filter_commit(transaction, filter, head)?;
    Ok(transaction.repo().find_commit(filtered)?.tree()?.id())
}

/// Build a commit whose tree holds `size` files under `content/`, generate an N_COMMITS history
/// that churns the to-be-replaced files, and construct the one-tree replacement filter (and, when
/// `with_per_blob` is set, the per-blob one too) over a fixed ~10% subset.
fn build_case(
    repo: &git2::Repository,
    size: usize,
    with_per_blob: bool,
) -> anyhow::Result<SizeCase> {
    // Distribute files uniformly across nested subfolders, all under CONTENT_DIR.
    let mut rng = StdRng::seed_from_u64(0);
    let files_in_folder =
        rand::distr::Uniform::try_from(N_PER_SUBFOLDER_MIN..=N_PER_SUBFOLDER_MAX)?;

    let mut builder = git2::build::TreeUpdateBuilder::new();
    let mut all_paths = vec![];

    while all_paths.len() < size {
        let mut subpath = PathBuf::from(CONTENT_DIR);
        for _ in 0..NESTING_LEVEL {
            subpath.push(random_string(&mut rng, PATH_COMPONENT_LENGTH));
        }

        let to_add = files_in_folder.sample(&mut rng).min(size - all_paths.len());
        for i in 0..to_add {
            let file_name = format!("file_{}", i);
            let full_path = subpath.join(&file_name);
            // Content = name; realistically a repo won't have many identical files, and these are
            // the blobs the filters replace anyway.
            let oid = repo.blob(file_name.as_bytes())?;
            builder.upsert(&full_path, oid, git2::FileMode::Blob);
            all_paths.push(full_path);
        }
    }

    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let root_tree = repo.find_tree(builder.create_updated(repo, &baseline)?)?;

    let sig = josh_commit_signature()?;
    // No ref update -- the commit oid is tracked directly per case.
    let initial = repo.commit(None, &sig, &sig, "content", &root_tree, &[])?;

    // Deterministically pick ~REPLACE_FRACTION of the paths to replace, and the single fixed blob
    // every replacement points at.
    use rand::RngExt;
    let mut pick = StdRng::seed_from_u64(1);
    let replaced_paths = all_paths
        .iter()
        .filter(|_| pick.random_bool(REPLACE_FRACTION))
        .cloned()
        .collect::<Vec<_>>();
    let fixed_blob = repo.blob(b"REPLACED")?;

    // Generate history on top of the initial commit; only the replaced files churn, so the head's
    // non-replaced remainder equals the initial one (which the one-tree result subtree relies on).
    let head = build_history(repo, initial, &replaced_paths)?;

    // Approach A: one Op::Insert per replaced file, all composed together. The inserts are combined
    // into a single filter, then merged with the exclude of that same combined filter: the exclude
    // drops the originals at those paths so the (generative) inserts can re-add them without
    // dropping siblings (the proven `compose([exclude[insert], insert])` replacement pattern,
    // generalized to the whole replaced set). This is the per-op-heavy approach whose cost scales
    // with the number of replaced files. Skipped for one-tree-only sizes.
    let filter_per_blob = if with_per_blob {
        let inserts = replaced_paths
            .iter()
            .map(|path| josh_filter::Filter::new().insert_oid(path, fixed_blob))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let combined = josh_filter::compose(&inserts);
        Some(josh_filter::compose(&[
            josh_filter::Filter::new().exclude(combined),
            combined,
        ]))
    } else {
        None
    };

    // Approach B: pre-build the whole result subtree (original content/ with the replaced paths
    // swapped to the fixed blob) and place it with a single insert of that tree oid at `content`.
    let content_tree = root_tree.get_path(&PathBuf::from(CONTENT_DIR))?.id();
    let content_tree = repo.find_tree(content_tree)?;
    let mut result_builder = git2::build::TreeUpdateBuilder::new();
    for path in &replaced_paths {
        let relative = path.strip_prefix(CONTENT_DIR)?;
        result_builder.upsert(relative, fixed_blob, git2::FileMode::Blob);
    }
    let result_subtree = result_builder.create_updated(repo, &content_tree)?;
    let filter_one_tree = josh_filter::Filter::new().insert_oid(CONTENT_DIR, result_subtree)?;

    Ok(SizeCase {
        size,
        head,
        filter_per_blob,
        filter_one_tree,
    })
}

/// Generate N_COMMITS commits on top of `initial`, each re-randomizing a random share of the
/// replaced files (and only those). This gives filter_commit a real history to walk while keeping
/// every commit's non-replaced content identical to the initial commit.
fn build_history(
    repo: &git2::Repository,
    initial: git2::Oid,
    replaced_paths: &[PathBuf],
) -> anyhow::Result<git2::Oid> {
    use rand::RngExt;
    let mut rng = StdRng::seed_from_u64(2);
    let mut head = initial;

    for i in 0..N_COMMITS {
        let parent = repo.find_commit(head)?;
        let tree = parent.tree()?;
        let mut builder = git2::build::TreeUpdateBuilder::new();

        for path in replaced_paths {
            if rng.random_bool(PROB_CHURN) {
                let content = random_string(&mut rng, CHURN_CONTENT_LEN);
                let blob = repo.blob(content.as_bytes())?;
                builder.upsert(path, blob, git2::FileMode::Blob);
            }
        }

        let new_tree = repo.find_tree(builder.create_updated(repo, &tree)?)?;
        let sig = josh_commit_signature()?;
        head = repo.commit(
            None,
            &sig,
            &sig,
            &format!("commit {i}"),
            &new_tree,
            &[&parent],
        )?;
    }

    Ok(head)
}

fn ultrawide_insert(c: &mut Criterion) {
    // The insert_oid builder is gated behind the experimental-features flag, read once via a
    // LazyLock. Enable it before building any filter. (set_var is unsafe under Rust 2024.)
    unsafe {
        std::env::set_var("JOSH_EXPERIMENTAL_FEATURES", "1");
    }

    // Print `bench`-target span durations to stderr; this directive leaves josh_core's filter_commit
    // span (which records the filter as a field) suppressed, so no filter is ever printed.
    josh_test_support::init_tracing("bench=trace");

    let bench = InsertBench::setup().expect("set up benchmark");

    let mut group = c.benchmark_group("ultrawide_insert");
    // The per-blob approach costs tens of seconds per iteration at the largest tree, so keep
    // Criterion at its minimum sample count to bound the total wall-clock of a run.
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(case.size as u64));

        // The per-blob variant is only present for sizes where it is not prohibitively slow.
        let mut variants = Vec::new();
        if let Some(filter) = case.filter_per_blob {
            variants.push(("per_blob", filter));
        }
        variants.push(("one_tree", case.filter_one_tree));

        for (name, filter) in variants {
            group.bench_function(BenchmarkId::new(name, case.size), |b| {
                b.iter_with_setup_wrapper(|runner| {
                    // Per-iteration setup (untimed): start from a cold cache and a fresh transaction
                    // so every run does the full filtering work instead of hitting memoized results.
                    josh_core::reset_caches().expect("reset caches");
                    let transaction = bench.context.open().expect("open transaction");

                    // Optional: route object writes through an in-memory mempack backend to measure
                    // the ODB write-path overhead. The odb is bound first so it outlives the mempack.
                    let mempack_odb = std::env::var_os("JOSH_BENCH_MEMPACK")
                        .map(|_| transaction.repo().odb().expect("odb"));
                    let _mempack = mempack_odb.as_ref().map(|odb| {
                        odb.add_new_mempack_backend(1000)
                            .expect("add mempack backend")
                    });

                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();

                    runner.run(|| {
                        josh_core::filter_commit(&transaction, filter, case.head)
                            .expect("filter commit")
                    });

                    drop(iter_span);
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, ultrawide_insert);
criterion_main!(benches);
