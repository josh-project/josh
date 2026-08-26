use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use josh_core::git::josh_actor_signature;
use josh_test_support::bench::EntryKind;
use rand::prelude::*;
use std::cell::RefCell;
use std::ops::DerefMut;
use std::str::FromStr;

const N_FILES: usize = if cfg!(debug_assertions) { 50 } else { 500 };

const N_COMMITS: usize = if cfg!(debug_assertions) { 5 } else { 10 };

const N_PER_SUBFOLDER_MIN: usize = 10;
const N_PER_SUBFOLDER_MAX: usize = 100;

const NESTING_LEVEL: usize = 3;

/// Expected head oid of the cached bench repo. This is the cache validity key:
/// changing any of the build parameters above changes the produced oid, which
/// then fails the strict check in `provision_repo` and reports the new value to
/// paste here. Filled in by running the bench once after a build change.
const EXPECTED_HEAD: &str = "dec8381c11bbbf281cfec78d8bf9b19283971f99";

/// Fixed commit timestamp fed to `josh_actor_signature()` via `JOSH_COMMIT_TIME`
/// so the built history is reproducible. Without it the signature uses the wall
/// clock, every run produces a different head oid, and `EXPECTED_HEAD` can never
/// be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

struct PinBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of
    // the benchmark.
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    filter: josh_core::filter::Filter,
    head: gix_hash::ObjectId,
}

impl PinBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oid is reproducible
        // and `EXPECTED_HEAD` stays valid across runs. Must run before the cache
        // miss path invokes the build callback.
        // SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        // Build (or reuse from cache) the bare repo. The `build_initial_state` /
        // `build_history` pair runs only inside the callback on a cache miss; the
        // resulting head oid is checked against `EXPECTED_HEAD` before the repo is
        // cached and copied into a tempdir for this run.
        let provisioned = josh_test_support::provision_repo::provision_repo(
            "ultrawide_pin",
            &gix_hash::ObjectId::from_str(EXPECTED_HEAD)
                .expect("EXPECTED_HEAD must be a valid oid"),
            |repo| {
                let (head, paths) = tracing::info_span!(target: "bench", "build_initial_state")
                    .in_scope(|| build_initial_state(repo))?;

                let head =
                    tracing::info_span!(target: "bench", "build_history", n_paths = paths.len())
                        .in_scope(|| build_history(repo, &paths, head))?;

                Ok(head)
            },
        )?;

        let head = provisioned.head;

        // Pin the sled db open across iterations: per-iteration transaction drops would
        // otherwise close and reopen it, and the cycle's flush/reopen I/O dominates short cases.
        let sled = josh_core::cache::SledCacheBackend::new(provisioned.path());
        sled.pin()?;
        let cache = std::sync::Arc::new(josh_core::cache::CacheStack::new().with_backend(sled));

        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // The filter under benchmark: select the workspace defined by the
        // `workspace/workspace.josh` files generated throughout the history.
        let filter = josh_core::filter::Filter::new()
            .stored("workspace/workspace")
            .with_meta("history", "no-splice");

        Ok(Self {
            _repo: provisioned,
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
    repo: &gix::Repository,
) -> anyhow::Result<(gix_hash::ObjectId, Vec<std::path::PathBuf>)> {
    const PATH_COMPONENT_LENGTH: usize = 15;

    // Create multiple nested subfolders in the benchmark repo; aiming for a uniform
    // distribution of a number of files in each subfolder.
    let mut rng = StdRng::seed_from_u64(0);
    let files_in_folder =
        rand::distr::Uniform::try_from(N_PER_SUBFOLDER_MIN..=N_PER_SUBFOLDER_MAX)?;

    let baseline = josh_test_support::bench::empty_tree(repo)?;
    let mut builder = repo.edit_tree(baseline)?;
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
            let oid = josh_gix_ext::write_blob(&repo.objects, file_name.as_bytes())?;

            all_paths.push(full_path.clone());
            builder.upsert(
                full_path.to_str().expect("benchmark paths are UTF-8"),
                EntryKind::Blob,
                oid,
            )?;
        }

        total_files += to_add;
    }

    // Seed an initial workspace selecting every file (no pins yet) so a workspace
    // exists from the very first commit. Without it, the workspace filter would
    // resolve to empty for the root commit and its descendants would have an
    // empty filtered parent.
    let workspace = josh_filter::as_file(paths_to_compose(&all_paths), 2);
    let blob = josh_gix_ext::write_blob(&repo.objects, workspace.as_bytes())?;
    builder.upsert("workspace/workspace.josh", EntryKind::Blob, blob)?;

    let new_tree = builder.write()?.detach();

    let sig = josh_actor_signature()?;
    let head =
        josh_gix_ext::write_commit(&repo.objects, new_tree, &[], &sig, &sig, "initial commit")?;
    repo.reference(
        "refs/heads/main",
        head,
        gix::refs::transaction::PreviousValue::Any,
        "initial commit",
    )?;

    Ok((head, all_paths))
}

fn build_history(
    repo: &gix::Repository,
    paths: &[std::path::PathBuf],
    mut head: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
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
        let parent = josh_gix_ext::CommitData::read(&repo.objects, head)?;
        let tree = parent.tree_id()?;
        let mut builder = repo.edit_tree(tree)?;

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
            let blob = josh_gix_ext::write_blob(&repo.objects, updated_content.as_bytes())?;

            builder.upsert(
                updated_path.to_str().expect("benchmark paths are UTF-8"),
                EntryKind::Blob,
                blob,
            )?;
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
        let blob = josh_gix_ext::write_blob(&repo.objects, workspace.as_bytes())?;

        builder.upsert("workspace/workspace.josh", EntryKind::Blob, blob)?;

        // Commit the updated tree on top of the current head.
        let new_tree = builder.write()?.detach();

        let sig = josh_actor_signature()?;
        head = josh_gix_ext::write_commit(
            &repo.objects,
            new_tree,
            &[head],
            &sig,
            &sig,
            &format!("commit {i_commit}"),
        )?;
        repo.reference(
            "refs/heads/main",
            head,
            gix::refs::transaction::PreviousValue::Any,
            "bench commit",
        )?;
    }

    Ok(head)
}

fn ultrawide_pin(c: &mut Criterion) {
    // Print span durations to stderr; only `bench`-target spans unless RUST_LOG overrides.
    josh_test_support::init_tracing("bench=trace");

    let bench = PinBench::setup().expect("set up benchmark");

    c.bench_function("ultrawide_filter_pin", |b| {
        b.iter_batched(
            // Per-iteration setup (untimed): start from a cold cache and a fresh
            // transaction so every run does the full filtering work instead of
            // hitting memoized results.
            || {
                josh_core::reset_caches().expect("reset caches");
                bench.context.open().expect("open transaction")
            },
            // Timed: filter the head commit. The transaction is returned so it is
            // dropped untimed after the measured section.
            |transaction| {
                let iter_span = tracing::info_span!(target: "bench", "iter").entered();

                josh_core::filter_commit(&transaction, bench.filter, bench.head)
                    .expect("filter commit");

                drop(iter_span);
                transaction
            },
            BatchSize::PerIteration,
        );
    });
}

criterion_group!(benches, ultrawide_pin);
criterion_main!(benches);
