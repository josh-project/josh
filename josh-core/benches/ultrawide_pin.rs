use criterion::{Criterion, criterion_group, criterion_main};
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::cell::RefCell;
use std::ops::DerefMut;

const N_FILES: usize = if cfg!(debug_assertions) { 50 } else { 500 };

const N_COMMITS: usize = if cfg!(debug_assertions) { 5 } else { 10 };

const N_PER_SUBFOLDER_MIN: usize = 10;
const N_PER_SUBFOLDER_MAX: usize = 100;

const NESTING_LEVEL: usize = 3;

struct PinBench {
    // Keeps the on-disk repository alive for the duration of the benchmark.
    _tmp: tempfile::TempDir,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    filter: josh_core::filter::Filter,
    head: git2::Oid,
}

impl PinBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        let tmp = tempfile::tempdir()?;
        let repo = git2::Repository::init_bare(tmp.path())?;

        let (head, paths) = tracing::info_span!(target: "bench", "build_initial_state")
            .in_scope(|| build_initial_state(&repo))?;

        let head = tracing::info_span!(target: "bench", "build_history", n_paths = paths.len())
            .in_scope(|| build_history(&repo, &paths, head))?;

        josh_core::cache::sled_load(tmp.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );

        let context = josh_core::cache::TransactionContext::new(tmp.path(), cache);

        // The filter under benchmark: select the workspace defined by the
        // `workspace/workspace.josh` files generated throughout the history.
        let filter = josh_core::filter::Filter::new().workspace("workspace");

        Ok(Self {
            _tmp: tmp,
            context,
            filter,
            head,
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

fn paths_to_compose(paths: &[std::path::PathBuf]) -> josh_filter::Filter {
    josh_filter::compose(
        &paths
            .iter()
            .map(|path| josh_filter::Filter::new().file(path))
            .collect::<Vec<_>>(),
    )
}

fn build_initial_state(
    repo: &git2::Repository,
) -> anyhow::Result<(git2::Oid, Vec<std::path::PathBuf>)> {
    const PATH_COMPONENT_LENGTH: usize = 15;

    // Create multiple nested subfolders in the benchmark repo; aiming for a uniform
    // distribution of a number of files in each subfolder.
    let mut rng = StdRng::seed_from_u64(0);
    let files_in_folder =
        rand::distr::Uniform::try_from(N_PER_SUBFOLDER_MIN..=N_PER_SUBFOLDER_MAX)?;

    let mut builder = git2::build::TreeUpdateBuilder::new();
    let mut all_paths = vec![];
    let mut total_files = 0usize;

    while total_files < N_FILES {
        let to_add = files_in_folder.sample(&mut rng);

        let subpath = (0..NESTING_LEVEL)
            .map(|_| random_string(&mut rng, PATH_COMPONENT_LENGTH))
            .collect::<std::path::PathBuf>();

        for i in 0..to_add {
            let file_name = format!("file_{}", i);
            let full_path = subpath.join(&file_name);

            // Use same content as name, realistically there won't
            // be that many identical files in a repo
            //
            // In subsequent commits will be rewritten anyway to
            // simulate pinned updates
            let oid = repo.blob(file_name.as_bytes())?;

            all_paths.push(full_path.clone());
            builder.upsert(full_path, oid, git2::FileMode::Blob);
        }

        total_files += to_add;
    }

    // Seed an initial workspace selecting every file (no pins yet) so a workspace
    // exists from the very first commit. Without it, the workspace filter would
    // resolve to empty for the root commit and its descendants would have an
    // empty filtered parent.
    let workspace = josh_filter::as_file(paths_to_compose(&all_paths), 2);
    let blob = repo.blob(workspace.as_bytes())?;
    builder.upsert("workspace/workspace.josh", blob, git2::FileMode::Blob);

    let baseline = repo.treebuilder(None)?.write()?;
    let baseline = repo.find_tree(baseline)?;

    let new_tree = builder.create_updated(&repo, &baseline)?;
    let new_tree = repo.find_tree(new_tree)?;

    let sig = josh_commit_signature()?;
    let head = repo.commit(
        Some("refs/heads/main"),
        &sig,
        &sig,
        "initial commit",
        &new_tree,
        &[],
    )?;

    Ok((head, all_paths))
}

fn build_history(
    repo: &git2::Repository,
    paths: &[std::path::PathBuf],
    mut head: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    use rand::RngExt;

    // In every commit, we update 10% files in the repo
    const PROB_FILE_UPDATED: f64 = 0.1;

    // There's 25% probability the file will end up on hold
    const PROB_UPDATE_ON_HOLD: f64 = 0.25;

    // Shouldn't matter for this benchmark, we don't look into blobs
    const BLOB_CONTENT_LEN: usize = 10;

    let rng = RefCell::new(StdRng::seed_from_u64(0));
    let include_path = || rng.borrow_mut().random_bool(PROB_FILE_UPDATED);
    let hold_off = || rng.borrow_mut().random_bool(PROB_UPDATE_ON_HOLD);

    // Built once: it is the same across all revisions, and the per-commit `:pin`
    // is layered on top of it below.
    let wide = paths_to_compose(paths);

    // The set of currently held-back paths, carried across revisions. Unchanged paths keep
    // their pin status; only updated paths re-roll it below. Starts empty, matching the
    // pin-less workspace seeded into the initial commit.
    let mut pinned = std::collections::BTreeSet::<std::path::PathBuf>::new();

    for i_commit in 0..N_COMMITS {
        let parent = repo.find_commit(head)?;
        let tree = parent.tree()?;
        let mut builder = git2::build::TreeUpdateBuilder::new();

        let updated_paths = paths
            .iter()
            .filter(|_| include_path())
            .cloned()
            .collect::<Vec<_>>();

        for updated_path in &updated_paths {
            // Re-roll the pin status of every changed path; paths left untouched keep the
            // status they carried over from the previous revision. Because each changed
            // path is held with PROB_UPDATE_ON_HOLD, an update can flip a path from pinned
            // to unpinned (or vice versa), and the overall held-back fraction settles
            // near 25% for the batch of files being updated per-commit.
            if hold_off() {
                pinned.insert(updated_path.clone());
            } else {
                pinned.remove(updated_path);
            }

            let updated_content = random_string(rng.borrow_mut().deref_mut(), BLOB_CONTENT_LEN);
            let blob = repo.blob(updated_content.as_bytes())?;

            builder.upsert(updated_path, blob, git2::FileMode::Blob);
        }

        // Pin the entire held-back set on top of the wide compose so those paths' updates
        // are held back across this revision.
        let pinned_filters = pinned
            .iter()
            .map(|path| josh_filter::Filter::new().file(path))
            .collect::<Vec<_>>();

        let filter = wide.pin(josh_filter::compose(&pinned_filters));

        // Render the filter into `workspace/workspace.josh` and add it to the
        // tree we're preparing for this commit.
        let workspace = josh_filter::as_file(filter, 2);
        let blob = repo.blob(workspace.as_bytes())?;

        builder.upsert("workspace/workspace.josh", blob, git2::FileMode::Blob);

        // Commit the updated tree on top of the current head.
        let new_tree = builder.create_updated(repo, &tree)?;
        let new_tree = repo.find_tree(new_tree)?;

        let sig = josh_commit_signature()?;
        head = repo.commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            &format!("commit {i_commit}"),
            &new_tree,
            &[&parent],
        )?;
    }

    Ok(head)
}

fn ultrawide_pin(c: &mut Criterion) {
    // Print span durations to stderr; only `bench`-target spans unless RUST_LOG overrides.
    josh_test_support::init_tracing("bench=trace");

    let bench = PinBench::setup().expect("set up benchmark");

    c.bench_function("ultrawide_filter_pin", |b| {
        b.iter_with_setup_wrapper(|runner| {
            // Per-iteration setup (untimed): start from a cold cache and a fresh
            // transaction so every run does the full filtering work instead of
            // hitting memoized results.
            josh_core::reset_caches().expect("reset caches");
            let transaction = bench.context.open(None).expect("open transaction");

            // Optional: route object writes through an in-memory mempack backend instead
            // of the loose-file backend, to measure the ODB write-path overhead. Reads of
            // the pre-built history fall through to the lower-priority loose backend. The
            // odb is bound first so it outlives the borrowed mempack handle.
            let mempack_odb = std::env::var_os("JOSH_BENCH_MEMPACK")
                .map(|_| transaction.repo().odb().expect("odb"));
            let _mempack = mempack_odb.as_ref().map(|odb| {
                odb.add_new_mempack_backend(1000)
                    .expect("add mempack backend")
            });

            let iter_span = tracing::info_span!(target: "bench", "iter").entered();

            runner.run(|| {
                josh_core::filter_commit(&transaction, bench.filter, bench.head)
                    .expect("filter commit")
            });

            drop(iter_span);
        });
    });
}

criterion_group!(benches, ultrawide_pin);
criterion_main!(benches);
