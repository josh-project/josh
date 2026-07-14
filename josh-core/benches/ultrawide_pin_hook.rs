use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// Tree sizes (number of files) benchmarked. Kept small in debug builds so `--test` runs stay fast.
// The pin parameter is a compose of one `file` filter per currently-pinned path, rebuilt for every
// commit, so cost grows with both the pinned-set size and the number of commits. Pin evaluation
// scales superlinearly (~n^1.7 here: 10k ~0.7s, 100k ~36s per iteration), so 100k is the practical
// top end -- larger trees push a full run into the hours even though the cache makes their setup
// cheap.
const SIZES: &[usize] = if cfg!(debug_assertions) {
    &[50, 500]
} else {
    &[100, 1_000, 10_000, 100_000]
};

// Number of history commits generated on top of the initial commit. The pinned set evolves per
// commit, so the pin parameter is rebuilt and re-applied at each of these.
const N_COMMITS: usize = if cfg!(debug_assertions) { 5 } else { 10 };

// Fraction of the tree's files that each commit changes ("churns"). Only churned files re-roll their
// hold status; the pinned set is otherwise carried across commits, so it stays similar from commit to
// commit with a few paths added or removed each time.
const CHURN_FRACTION: f64 = 0.1;
const CHURN_CONTENT_LEN: usize = 10;

// Probability that a churned file ends up held back (pinned). Each churned file re-rolls this, so a
// change can transition it from unpinned to pinned or vice versa, while unchurned files keep their
// status. Mirrors the model in the `ultrawide_pin` bench.
const PROB_UPDATE_ON_HOLD: f64 = 0.25;

const PATH_COMPONENT_LENGTH: usize = 15;
const NESTING_LEVEL: usize = 3;
const N_PER_SUBFOLDER_MIN: usize = 10;
const N_PER_SUBFOLDER_MAX: usize = 100;

// Largest size whose setup runs the sanity gate. The gate does a full untimed `filter_commit`, which
// costs tens of seconds at the top sizes; since the reconstruction and hook logic are
// size-independent, verifying on the smaller cases gives the same confidence, so larger cases are
// skipped to keep setup cheap.
const SANITY_GATE_MAX_SIZE: usize = 10_000;

// The two hook arguments, one per approach under comparison. The outer filter is `:hook=<arg>`, so
// `arg` in `filter_for_commit` selects which per-commit pin filter to serve.
//   - `per_path`: the pin argument is a compose of one `:file` filter per pinned path, rebuilt for
//     every commit. Filter-engine work grows with the pinned-set size.
//   - `one_tree`: the pin argument is a single `:$.=<oid>` insert of a precomputed tree that holds
//     exactly the pinned paths. Pin legalization intersects by path (`Select`/`Exclude` are
//     path-based), so this is equivalent to `per_path` while keeping the per-commit filter a single
//     op regardless of how many paths are pinned.
const HOOK_ARG_PER_PATH: &str = "per_path";
const HOOK_ARG_ONE_TREE: &str = "one_tree";

/// Expected oid of the cached bench repo's aggregate index commit. This is the cache validity key:
/// changing any build parameter above changes a case head, which changes the index commit oid, which
/// then fails the strict check in `provision_repo` and reports the new value to paste here. Filled in
/// by running the bench once after a build change.
const EXPECTED_HEAD: &str = "8deb73b572162ae4b4caf36ae52f3b2ef70e1485";

/// Fixed commit timestamp fed to `josh_commit_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces
/// different head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

// Why a hook (and not a plain `:pin` filter)? `:pin`'s hold-back logic lives in `per_rev_filter`,
// which josh only invokes for the per-revision filter sources -- `:workspace`, `:+stored`, and
// `:hook`. A bare `Filter::new().pin(..)` fed straight to `filter_commit` is a tree-level no-op
// (`Op::Pin(_) => Ok(x)`), so it would measure nothing. The original `ultrawide_pin` bench drives
// pin through a stored `workspace.josh`, which pays a filter parse+legalize per commit; the hook
// serves a pre-built per-commit filter by oid instead, isolating the pin evaluation itself.
struct BenchPinHook {
    per_path: HashMap<git2::Oid, josh_filter::Filter>,
    one_tree: HashMap<git2::Oid, josh_filter::Filter>,
}

impl josh_core::cache::FilterHook for BenchPinHook {
    fn filter_for_commit(
        &self,
        commit_oid: git2::Oid,
        arg: &str,
    ) -> anyhow::Result<josh_filter::Filter> {
        let map = match arg {
            HOOK_ARG_PER_PATH => &self.per_path,
            HOOK_ARG_ONE_TREE => &self.one_tree,
            _ => anyhow::bail!("unexpected pin hook arg: {arg}"),
        };
        map.get(&commit_oid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("no {arg} pin filter for commit {commit_oid}"))
    }
}

/// One tree size and the head of its generated history. The per-commit pin filters live in the
/// shared hook.
struct SizeCase {
    size: usize,
    head: git2::Oid,
}

struct PinBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark.
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
    // Attached to every opened transaction so `:hook=<arg>` resolves per commit.
    hook: Arc<BenchPinHook>,
    // The two outer filters under benchmark, both `:hook=<arg>` carrying the "no-splice" history
    // flag; they differ only in the per-commit pin argument the hook serves for each arg.
    filter_per_path: josh_filter::Filter,
    filter_one_tree: josh_filter::Filter,
}

impl PinBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oids are reproducible and
        // `EXPECTED_HEAD` stays valid across runs. Must run before the cache-miss path invokes the
        // build callback.
        // SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        // Build (or reuse from cache) the bare repo holding every size case. On a cache miss the
        // callback builds all cases, tags each tip with a `refs/heads/case_<size>` ref, and returns
        // an aggregate index commit whose oid is the content-addressed cache stamp checked against
        // `EXPECTED_HEAD`.
        let provisioned = josh_test_support::provision_repo::provision_repo(
            "ultrawide_pin_hook",
            &git2::Oid::from_str(EXPECTED_HEAD).expect("EXPECTED_HEAD must be a valid oid"),
            |repo| {
                let mut heads = vec![];
                for &size in SIZES {
                    let head = tracing::info_span!(target: "bench", "build_case", size)
                        .in_scope(|| build_case(repo, size))?;
                    heads.push(head);
                }
                build_index(repo, &heads)
            },
        )?;

        // Recover the size cases and per-commit pin filters from the provisioned repo. This runs
        // identically whether the repo was freshly built or copied from cache, so the pin filters
        // never depend on build-time state the cache would drop. Each commit's churned set is
        // recovered by diffing against its parent; `record_case` folds that into the evolving pinned
        // set that the pin filters are built from.
        let mut per_path = HashMap::new();
        let mut one_tree = HashMap::new();
        let mut cases = vec![];
        {
            let repo = &provisioned.repo;
            for &size in SIZES {
                let head = repo.refname_to_id(&format!("refs/heads/case_{size}"))?;
                record_case(repo, head, &mut per_path, &mut one_tree)?;
                cases.push(SizeCase { size, head });
            }
        }

        // `record_case` writes the `one_tree` pin-argument trees straight into the provisioned repo
        // as loose objects (`provision_repo` only packs what its build callback produced). The
        // `one_tree` filter reads those trees on every iteration, so leaving them loose makes the
        // benchmark measure loose-ODB lookups instead of filtering. Pack them into the repo's
        // single packfile, matching how `provision_repo` stores the rest of the history.
        repack(provisioned.path())?;

        let hook = Arc::new(BenchPinHook { per_path, one_tree });
        let filter_per_path = josh_filter::Filter::new()
            .hook(HOOK_ARG_PER_PATH)
            .with_meta("history", "no-splice");
        let filter_one_tree = josh_filter::Filter::new()
            .hook(HOOK_ARG_ONE_TREE)
            .with_meta("history", "no-splice");

        josh_core::cache::sled_load(provisioned.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // Sanity + correctness gate (untimed): confirm the `per_path` filter actually holds paths
        // back (so we never silently measure a no-op pin), and that the `one_tree` approach produces
        // an identical filtered tree (so the two variants are genuinely equivalent and the timing
        // comparison is fair). Because the pinned set churns, the filtered head tree must differ from
        // the raw head tree. Run through a throwaway transaction, then reset caches so nothing here
        // warms the timed runs. Only the cases up to `SANITY_GATE_MAX_SIZE` are checked; the gate is
        // size-independent, and a full `filter_commit` on the largest trees would dominate setup.
        {
            let transaction = context.open()?.with_filter_hook(hook.clone());
            for case in cases.iter().filter(|c| c.size <= SANITY_GATE_MAX_SIZE) {
                let per_path = josh_core::filter_commit(&transaction, filter_per_path, case.head)?;
                let one_tree = josh_core::filter_commit(&transaction, filter_one_tree, case.head)?;
                let per_path_tree = transaction.repo().find_commit(per_path)?.tree()?.id();
                let one_tree_tree = transaction.repo().find_commit(one_tree)?.tree()?.id();
                let raw_tree = transaction.repo().find_commit(case.head)?.tree()?.id();
                anyhow::ensure!(
                    per_path_tree != raw_tree,
                    "pin had no visible effect for size {} -- benchmark would measure a no-op",
                    case.size
                );
                anyhow::ensure!(
                    per_path_tree == one_tree_tree,
                    "per_path and one_tree pin approaches disagree for size {}",
                    case.size
                );
            }
        }
        josh_core::reset_caches()?;

        Ok(Self {
            _repo: provisioned,
            context,
            cases,
            hook,
            filter_per_path,
            filter_one_tree,
        })
    }
}

/// Pack every loose object in the repo at `path` into a single packfile and drop the loose copies,
/// so the timed iterations read from the packfile rather than paying per-object loose lookups. The
/// pin-argument trees are not reachable from any ref, so `--keep-unreachable` is required -- a plain
/// `git repack -a -d` only packs ref-reachable objects and would leave those trees loose.
fn repack(path: &std::path::Path) -> anyhow::Result<()> {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["repack", "-a", "-d", "--keep-unreachable"])
        .status()?;
    anyhow::ensure!(status.success(), "git repack failed with status {status}");
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

/// Approach A's per-commit pin filter: pin the identity tree by a compose of one `file` filter per
/// pinned path (an empty pin parameter when nothing is pinned). The pin argument grows one op per
/// pinned path, so filter-engine work scales with the pinned-set size.
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

/// Approach B's per-commit pin filter: pin the identity tree by a single `:$.=<oid>` insert of a
/// precomputed tree holding exactly the pinned paths. Pin legalization intersects by path
/// (`Select`/`Exclude` subtract path-by-path, independent of blob content), so only the tree's set
/// of paths matters -- every path gets the same shared placeholder blob, and an empty tree stands in
/// for the empty pinned set. The pin argument is one op no matter how many paths are pinned, so
/// filter-engine work stays near-constant as the pinned set grows.
fn pin_filter_tree(
    repo: &git2::Repository,
    pinned: &[PathBuf],
) -> anyhow::Result<josh_filter::Filter> {
    let placeholder = repo.blob(b"x")?;
    let mut builder = git2::build::TreeUpdateBuilder::new();
    for path in pinned {
        builder.upsert(path, placeholder, git2::FileMode::Blob);
    }
    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let tree = builder.create_updated(repo, &baseline)?;
    let param = josh_filter::Filter::new().insert_oid(".", tree)?;
    Ok(josh_filter::Filter::new().pin(param))
}

/// Build a commit whose tree holds `size` files, then generate an N_COMMITS history that churns files
/// per commit. The tip is tagged with `refs/heads/case_<size>` so the head is recoverable after the
/// repo round-trips through the cache; the pinned set and per-commit pin filters are not recorded here
/// -- they are reconstructed from tree diffs in `record_case`.
fn build_case(repo: &git2::Repository, size: usize) -> anyhow::Result<git2::Oid> {
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
    // No ref update yet -- the tip ref is set once the history is complete.
    let mut head = repo.commit(None, &sig, &sig, "content", &root_tree, &[])?;

    // Deterministic history: each commit churns a fresh random ~CHURN_FRACTION of the files. Only the
    // content churn is written here; the churned set is what `record_case` recovers by diffing against
    // the parent, and the evolving pinned set is derived from it there.
    let mut rng = StdRng::seed_from_u64(1);
    for i in 0..N_COMMITS {
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

    // Tag the tip so `record_case` can find this case's head on the cache-hit path, where the
    // build callback never runs. Also keeps the whole history reachable through `git prune`.
    repo.reference(
        &format!("refs/heads/case_{size}"),
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

/// Walk a case's linear history root-to-tip, reconstructing each commit's pinned set and recording
/// both approaches' pin filters (`per_path` and `one_tree`) keyed by commit oid. The two maps hold
/// equivalent filters for the same pinned set, differing only in how the pin argument is expressed.
/// Each commit's churned set is recovered by diffing against its
/// parent; every churned path then re-rolls its hold status via `holds_back`, evolving a pinned set
/// that is carried across commits (unchurned paths keep their status). This reproduces the intended
/// model purely from repo-derived data, so it is identical whether the repo was freshly built or
/// copied from cache.
fn record_case(
    repo: &git2::Repository,
    head: git2::Oid,
    per_path: &mut HashMap<git2::Oid, josh_filter::Filter>,
    one_tree: &mut HashMap<git2::Oid, josh_filter::Filter>,
) -> anyhow::Result<()> {
    // Collect the linear history oldest-first so the pinned set can be folded forward across commits.
    let mut chain = vec![];
    let mut oid = head;
    loop {
        let commit = repo.find_commit(oid)?;
        chain.push(oid);
        if commit.parent_count() == 0 {
            break;
        }
        oid = commit.parent_id(0)?;
    }
    chain.reverse();

    let mut pinned = std::collections::BTreeSet::<PathBuf>::new();
    for (commit_index, &oid) in chain.iter().enumerate() {
        let commit = repo.find_commit(oid)?;

        // Re-roll the hold status of every churned path; unchurned paths keep the status they carried
        // over. The root commit has no parent, so its churned set is empty and the pinned set stays
        // empty there.
        for path in changed_paths(repo, &commit)? {
            if holds_back(&path, commit_index) {
                pinned.insert(path);
            } else {
                pinned.remove(&path);
            }
        }

        let pins = pinned.iter().cloned().collect::<Vec<_>>();
        per_path.insert(oid, pin_filter(&pins));
        one_tree.insert(oid, pin_filter_tree(repo, &pins)?);
    }
    Ok(())
}

/// Deterministic hold decision for a churned `path` at commit position `commit_index`: `true` (held
/// back / pinned) with probability `PROB_UPDATE_ON_HOLD`. It is a pure function of repo-derived inputs
/// so `record_case` reproduces the same pinned-set evolution on every run, cache hit or miss. A fixed
/// FNV-1a hash seeds `StdRng` -- portable across platforms and Rust versions, unlike the standard
/// library's hasher -- and folding in `commit_index` lets the same path flip pinned/unpinned as
/// history advances.
fn holds_back(path: &std::path::Path, commit_index: usize) -> bool {
    use rand::RngExt;

    let mut hash = 0xcbf2_9ce4_8422_2325u64; // FNV-1a 64-bit offset basis
    for byte in path.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3); // FNV-1a 64-bit prime
    }
    hash ^= commit_index as u64;

    StdRng::seed_from_u64(hash).random_bool(PROB_UPDATE_ON_HOLD)
}

/// The paths that differ between `commit` and its first parent (empty for a parentless commit).
fn changed_paths(repo: &git2::Repository, commit: &git2::Commit) -> anyhow::Result<Vec<PathBuf>> {
    if commit.parent_count() == 0 {
        return Ok(vec![]);
    }
    let parent_tree = commit.parent(0)?.tree()?;
    let tree = commit.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)?;
    let mut paths = vec![];
    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path() {
                paths.push(path.to_path_buf());
            }
            true
        },
        None,
        None,
        None,
    )?;
    Ok(paths)
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

        // Both approaches are benchmarked at every size so the per-path compose and the single-tree
        // insert can be compared directly across the scaling parameter.
        let variants = [
            ("per_path", bench.filter_per_path),
            ("one_tree", bench.filter_one_tree),
        ];

        for (name, filter) in variants {
            group.bench_function(BenchmarkId::new(name, case.size), |b| {
                b.iter_with_setup_wrapper(|runner| {
                    // Per-iteration setup (untimed): start from a cold cache and a fresh transaction
                    // so every run does the full filtering work instead of hitting memoized results.
                    // The hook is re-attached because it lives on the transaction, not the context.
                    josh_core::reset_caches().expect("reset caches");
                    let transaction = bench
                        .context
                        .open()
                        .expect("open transaction")
                        .with_filter_hook(bench.hook.clone());

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

criterion_group!(benches, ultrawide_pin_hook);
criterion_main!(benches);
