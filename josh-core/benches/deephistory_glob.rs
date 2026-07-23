use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::Filter;
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::path::{Path, PathBuf};

// The scaling parameter of this benchmark is history *length*, not tree width. `filter_commit`
// walks and rewrites a commit's whole ancestry, so applying a filter to the head does O(history)
// work; this bench measures how a `::<glob>` pattern filter (Op::Pattern) scales as the number of
// commits grows while the tree stays a fixed, modest size. The hot path under test is the
// per-apply `glob::Pattern::new` compile plus the `tree::remove_pred` walk in the Op::Pattern arm
// of `apply_to_tree`. It is the pattern-filter counterpart of `deephistory_subdir`; the
// `widetree_glob` bench holds history short and grows the tree instead.
//
// Number of history commits generated on top of the root commit for each case. Kept small in debug
// builds so `cargo test`/`--test` runs stay fast.
const HISTORY_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[10, 100]
} else {
    &[100, 1_000, 10_000]
};

// A fixed, modest tree shared as the root of every case. This bench varies history length, not
// tree width, so the tree stays small: TREE_FILES files spread evenly across N_DIRS top-level
// directories (`dir_00`..`dir_{N_DIRS-1}`), each holding TREE_FILES / N_DIRS files. Half the files
// are `.rs`, half `.txt` (see `file_path`), so the recursive pattern matches ~half the blobs. On
// top of that grid:
//
// - `dir_00/sub/` holds N_SUB_FILES churned `.c` files (`c_0.c`..), the domain of the literal
//   pattern below.
// - `dir_01` holds the dotfile tripwire: a blob `dir_01/.hidden.rs` and a subtree
//   `dir_01/.hiddendir/inner.rs`. The pattern filter runs the glob crate with
//   `require_literal_leading_dot`, under which `*`/`**` do NOT match dot-leading components, so
//   both must be EXCLUDED by `::**/*.rs`; the correctness gates pin that down by using
//   `glob::Pattern::matches_path_with` (with exactly the Op::Pattern MatchOptions) as the
//   reference matcher.
//
// INVARIANT: `dir_00` stays completely dot-free. That way `::dir_00/**` keeps the `dir_00`
// subtree verbatim (its filtered subtree oid equals the raw one), which is exactly the shape an
// identity/subtree fast path in a remove_pred specialization would exploit -- while `dir_01`
// keeps the dotfile semantics honest at the same time.
const TREE_FILES: usize = 200;
const N_DIRS: usize = 10;
const N_SUB_FILES: usize = 5;

// Fraction of the tree's files each commit changes ("churns"). This keeps most commits touching
// files matched by every benchmarked pattern (the `dir_00/sub` `.c` files churn too), so
// filtering does not collapse any pattern's history to a handful of commits.
const CHURN_FRACTION: f64 = 0.1;
const CHURN_CONTENT_LEN: usize = 10;

// The three benchmarked patterns. The recursive one (`::**/*.rs`) matches ~half the blobs in
// every directory and forces a full-tree walk; the prefix one (`::dir_00/**`) is confined to a
// single top-level directory and is the case a prefix-pruning specialization of `remove_pred`
// should speed up most; the literal one (`::dir_00/sub/*.c`) has a fully literal directory prefix
// and only globs the final component -- benching them separately keeps before/after deltas
// attributable.
const PATTERN_RECURSIVE: &str = "**/*.rs";
const PATTERN_PREFIX: &str = "dir_00/**";
const PATTERN_LITERAL: &str = "dir_00/sub/*.c";
const PREFIX_DIR: &str = "dir_00";

/// Expected oid of the cached bench repo's aggregate index commit. This is the cache validity key:
/// changing any build parameter above changes a case head, which changes the index commit oid,
/// which then fails the strict check in `provision_repo` and reports the new value to paste here.
/// Filled in by running the bench once after a build change. Debug builds use reduced
/// HISTORY_SIZES, so they have their own expected oid and their own provision-cache name (below)
/// -- otherwise `cargo test --benches` and `cargo bench` would fight over the same cache entry.
const EXPECTED_HEAD: &str = if cfg!(debug_assertions) {
    "c058bc3feef644d6acb11bd05108601e6855a0a0"
} else {
    "f3fcede6e16cc8a4606bcf6a1ab21b2fa709cb48"
};

/// Provision-cache name, split per profile to match the per-profile EXPECTED_HEAD.
const CACHE_NAME: &str = if cfg!(debug_assertions) {
    "deephistory_glob_debug"
} else {
    "deephistory_glob"
};

/// Fixed commit timestamp fed to `josh_commit_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces
/// different head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One history length and the head of its generated history.
struct SizeCase {
    n_commits: usize,
    head: git2::Oid,
}

struct GlobBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark; the
    // incremental group also uses this handle to create per-iteration edit commits.
    repo: josh_test_support::provision_repo::ProvisionedRepo,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<SizeCase>,
}

impl GlobBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oids are reproducible and
        // `EXPECTED_HEAD` stays valid across runs. Must run before the cache-miss path invokes the
        // build callback. SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        // Build (or reuse from cache) the bare repo holding every history-length case. On a cache
        // miss the callback builds all cases, tags each tip with a `refs/heads/case_<n_commits>`
        // ref, and returns an aggregate index commit whose oid is the content-addressed cache
        // stamp checked against `EXPECTED_HEAD`.
        let provisioned = josh_test_support::provision_repo::provision_repo(
            CACHE_NAME,
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

        // Recover each case head from its ref. This runs identically whether the repo was freshly
        // built or copied from cache.
        let mut cases = vec![];
        {
            let repo = &provisioned.repo;
            for &n_commits in HISTORY_SIZES {
                let head = repo.refname_to_id(&format!("refs/heads/case_{n_commits}"))?;
                cases.push(SizeCase { n_commits, head });
            }
        }

        josh_core::cache::sled_load(provisioned.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // Correctness gates (untimed): confirm every benchmarked pattern filter produces exactly
        // the tree an independent git2 walk + reference glob match predicts, so we never silently
        // measure a filter that drops everything, is a no-op, or diverges from glob semantics. A
        // pattern filter keeps matching blobs at their ORIGINAL paths (it does not lift subtrees
        // to the root like `:/subdir`). The reference predicate is
        // `glob::Pattern::matches_path_with` with exactly the MatchOptions of the Op::Pattern arm
        // of `apply_to_tree`, so `require_literal_leading_dot` exclusion of `dir_01/.hidden.rs`
        // and `dir_01/.hiddendir` is checked automatically -- this is the dotfile-semantics
        // tripwire for remove_pred specializations. Run through a throwaway transaction on the
        // smallest case (the check is history-length independent), then reset caches so nothing
        // here warms the timed runs.
        {
            let transaction = context.open()?;
            let case = cases.first().expect("at least one case");
            let repo = transaction.repo();

            // The tripwire only means something if the dotfiles actually exist in the raw tree.
            let raw_tree = repo.find_commit(case.head)?.tree()?;
            for path in ["dir_01/.hidden.rs", "dir_01/.hiddendir/inner.rs"] {
                anyhow::ensure!(
                    raw_tree.get_path(Path::new(path)).is_ok(),
                    "raw head tree lost `{path}` -- dotfile gate would be vacuous"
                );
            }

            for pattern in [PATTERN_RECURSIVE, PATTERN_PREFIX, PATTERN_LITERAL] {
                let filter = Filter::new().pattern(pattern);
                let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
                let got = repo.find_commit(filtered)?.tree_id();
                let (want, kept) = expected_tree(repo, case.head, &glob_pred(pattern))?;
                anyhow::ensure!(
                    kept > 0,
                    "`::{pattern}` gate kept no blobs -- benchmark would be a no-op"
                );
                anyhow::ensure!(
                    got == want,
                    "`::{pattern}` produced {got}, expected {want} -- wrong measurement"
                );
            }

            // Stronger structural form for the prefix pattern: since `dir_00` is dot-free, the
            // result must be exactly one top-level entry, `PREFIX_DIR`, whose oid equals the raw
            // head's subtree oid ("original path preserved, subtree taken wholesale") -- the shape
            // an identity/subtree fast path must reproduce bit-identically.
            let filter = Filter::new().pattern(PATTERN_PREFIX);
            let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
            let got_tree = repo.find_commit(filtered)?.tree()?;
            anyhow::ensure!(
                got_tree.len() == 1,
                "`::{PATTERN_PREFIX}` result must have exactly one top-level entry"
            );
            let raw_subtree = raw_tree.get_path(Path::new(PREFIX_DIR))?.id();
            anyhow::ensure!(
                got_tree.get_name(PREFIX_DIR).map(|e| e.id()) == Some(raw_subtree),
                "`::{PATTERN_PREFIX}` must keep `{PREFIX_DIR}` at its original path"
            );
        }
        josh_core::reset_caches()?;

        Ok(Self {
            repo: provisioned,
            context,
            cases,
        })
    }
}

/// Path of file `i`: `dir_{i % N_DIRS}/file_{i}.{rs|txt}`. Files are spread round-robin across the
/// top-level directories; because N_DIRS is even, every directory holds an alternating mix of `.rs`
/// and `.txt` files (in dir_00: files 0, 10, 20, ... alternate rs/txt).
fn file_path(i: usize) -> PathBuf {
    let ext = if i % 2 == 0 { "rs" } else { "txt" };
    PathBuf::from(format!("dir_{:02}", i % N_DIRS)).join(format!("file_{i:04}.{ext}"))
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

/// Reference predicate for a pattern: match the full blob path with the glob crate under EXACTLY
/// the MatchOptions the Op::Pattern arm of `apply_to_tree` uses. Keeping the reference glob-based
/// (rather than a string stand-in) makes the correctness gates sensitive to every semantic detail,
/// most importantly `require_literal_leading_dot` (dot-leading components never match `*`/`**`).
fn glob_pred(pattern: &str) -> impl Fn(&str) -> bool {
    let pattern = glob::Pattern::new(pattern).expect("benchmarked pattern must be valid");
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };
    move |path: &str| pattern.matches_path_with(Path::new(path), options)
}

/// Rebuild, with plain git2 tree walking (no josh code), the tree the pattern filter must produce:
/// keep exactly the blobs whose full path satisfies `keep`, at their ORIGINAL paths (a pattern
/// filter preserves paths; it does not lift subtrees to the root). Returns the tree oid and the
/// number of kept blobs.
///
/// NOTE: a near-identical twin of this helper lives in `widetree_glob.rs` (criterion benches
/// cannot share a module without extra plumbing); keep the two copies in sync.
fn expected_tree(
    repo: &git2::Repository,
    head: git2::Oid,
    keep: &dyn Fn(&str) -> bool,
) -> anyhow::Result<(git2::Oid, usize)> {
    let tree = repo.find_commit(head)?.tree()?;
    let mut kept: Vec<(String, git2::Oid, i32)> = vec![];
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let path = format!("{}{}", root, entry.name().unwrap_or_default());
            if keep(&path) {
                kept.push((path, entry.id(), entry.filemode()));
            }
        }
        git2::TreeWalkResult::Ok
    })?;
    let mut builder = git2::build::TreeUpdateBuilder::new();
    for (path, oid, filemode) in &kept {
        let mode = match *filemode {
            0o100755 => git2::FileMode::BlobExecutable,
            0o120000 => git2::FileMode::Link,
            _ => git2::FileMode::Blob,
        };
        builder.upsert(Path::new(path), *oid, mode);
    }
    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    Ok((builder.create_updated(repo, &baseline)?, kept.len()))
}

/// Build a root commit whose tree holds `TREE_FILES` files spread across `N_DIRS` top-level
/// directories, then generate an `n_commits` history that churns ~`CHURN_FRACTION` of the files
/// per commit. The tip is tagged with `refs/heads/case_<n_commits>` so the head is recoverable
/// after the repo round-trips through the cache.
fn build_case(repo: &git2::Repository, n_commits: usize) -> anyhow::Result<git2::Oid> {
    use rand::RngExt;

    let mut builder = git2::build::TreeUpdateBuilder::new();
    let mut all_paths = vec![];
    for i in 0..TREE_FILES {
        let path = file_path(i);
        let oid = repo.blob(path.to_string_lossy().as_bytes())?;
        builder.upsert(&path, oid, git2::FileMode::Blob);
        all_paths.push(path);
    }

    // The literal pattern's domain: a nested subdir of churned `.c` files under dir_00 (which must
    // stay dot-free -- see the tree-shape invariant at the top of this file).
    for j in 0..N_SUB_FILES {
        let path = PathBuf::from("dir_00").join("sub").join(format!("c_{j}.c"));
        let oid = repo.blob(path.to_string_lossy().as_bytes())?;
        builder.upsert(&path, oid, git2::FileMode::Blob);
        all_paths.push(path);
    }

    // The dotfile tripwire under dir_01: one dot-leading blob and one dot-leading subtree. Static
    // (never churned) -- their role is purely to pin `require_literal_leading_dot` semantics in
    // the correctness gates.
    for path in ["dir_01/.hidden.rs", "dir_01/.hiddendir/inner.rs"] {
        let oid = repo.blob(path.as_bytes())?;
        builder.upsert(Path::new(path), oid, git2::FileMode::Blob);
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

    // Tag the tip so `setup` can find this case's head on the cache-hit path, where the build
    // callback never runs. Also keeps the whole history reachable through `git prune`.
    repo.reference(
        &format!("refs/heads/case_{n_commits}"),
        head,
        true,
        "bench case tip",
    )?;

    Ok(head)
}

/// Aggregate every case tip under one index commit. Its oid changes whenever any case head
/// changes, making it a faithful content-addressed cache stamp for the entire repo, and it keeps
/// all cases reachable so provision_repo's `git prune` retains the full history. It is never
/// filtered.
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

// A small fixed edit set for the incremental group, chosen so every commit touches both patterns'
// domains: indices {0, 1, 10, 11} map to dir_00/file_0000.rs, dir_01/file_0001.txt,
// dir_00/file_0010.rs and dir_01/file_0011.txt. A fifth rotating index is added per commit for
// variety.
const EDIT_INDICES: &[usize] = &[0, 1, 10, 11];

/// Create one child commit of `parent` that edits the fixed set plus one rotating file. Content is
/// derived from the monotonic counter `n` (NOT the wall clock) so results are deterministic; the
/// advancing parent chain guarantees every commit oid is unique, so no iteration is a pure cache
/// hit. New objects land only in this run's tempdir copy, never in the provision cache.
fn make_edit_commit(
    repo: &git2::Repository,
    parent: git2::Oid,
    n: u64,
) -> anyhow::Result<git2::Oid> {
    let parent = repo.find_commit(parent)?;
    let tree = parent.tree()?;
    let mut builder = git2::build::TreeUpdateBuilder::new();
    let blob = repo.blob(format!("local edit {n}").as_bytes())?;
    for &i in EDIT_INDICES {
        builder.upsert(&file_path(i), blob, git2::FileMode::Blob);
    }
    let rotating = ((n % TREE_FILES as u64) as usize * 7) % TREE_FILES;
    // TreeUpdateBuilder rejects duplicate paths, so skip the rotating edit when it happens to land
    // on one of the fixed indices.
    if !EDIT_INDICES.contains(&rotating) {
        builder.upsert(&file_path(rotating), blob, git2::FileMode::Blob);
    }
    let new_tree = repo.find_tree(builder.create_updated(repo, &tree)?)?;
    let sig = josh_commit_signature()?;
    Ok(repo.commit(
        None,
        &sig,
        &sig,
        &format!("local edit {n}"),
        &new_tree,
        &[&parent],
    )?)
}

fn deephistory_glob(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    // One shared setup for all three groups so the repo is provisioned only once per process.
    let bench = GlobBench::setup().expect("set up benchmark");

    // Group 1: cold-cache history scaling of the broad recursive pattern.
    let filter = Filter::new().pattern(PATTERN_RECURSIVE);
    let mut group = c.benchmark_group("deephistory_glob_recursive");
    // The longest history costs seconds per iteration, so keep Criterion at its minimum sample
    // count to bound the total wall-clock of a run.
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(case.n_commits as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_commits), |b| {
            b.iter_batched(
                // Per-iteration setup (untimed): start from a cold cache and a fresh transaction
                // so every run does the full filtering work -- including the per-apply
                // `glob::Pattern::new` compile and the full `remove_pred` walk, which are the
                // costs under test -- instead of hitting memoized results.
                || {
                    josh_core::reset_caches().expect("reset caches");
                    let transaction = bench.context.open().expect("open transaction");
                    let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                    (transaction, iter_span)
                },
                // Timed: filter the case head. The setup guards are returned so they are dropped
                // untimed after the measured section.
                |(transaction, iter_span)| {
                    josh_core::filter_commit(&transaction, filter, case.head)
                        .expect("filter commit");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();

    // Group 2: cold-cache history scaling of the prefix-prunable pattern.
    let filter = Filter::new().pattern(PATTERN_PREFIX);
    let mut group = c.benchmark_group("deephistory_glob_prefix");
    group.sample_size(10);
    for case in &bench.cases {
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
                    josh_core::filter_commit(&transaction, filter, case.head)
                        .expect("filter commit");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();

    // Group 3: cold-cache history scaling of the literal-prefix pattern (only the final component
    // globs).
    let filter = Filter::new().pattern(PATTERN_LITERAL);
    let mut group = c.benchmark_group("deephistory_glob_literal");
    group.sample_size(10);
    for case in &bench.cases {
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
                    josh_core::filter_commit(&transaction, filter, case.head)
                        .expect("filter commit");
                    (transaction, iter_span)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();

    // Group 4: warm-cache incremental filtering -- the proxy-serving scenario. The history is
    // already filtered with warm caches; one new commit with a small local edit arrives; measure
    // `filter_commit` of the new tip only. Caches are deliberately NEVER reset in this group.
    let patterns: &[(&str, &str)] = &[("recursive", PATTERN_RECURSIVE), ("prefix", PATTERN_PREFIX)];

    // Incremental correctness gate (untimed): build one probe commit the same way the iterations
    // do, filter it on a warm cache, and assert its filtered tree equals the independent
    // expectation -- this validates the warm/incremental path produces the same trees as the cold
    // path. The probe commits are discarded (each iteration below starts from the case head chain
    // independently).
    {
        let case = bench.cases.first().expect("at least one case");
        let transaction = bench.context.open().expect("open transaction");
        for (i, &(_, pattern)) in patterns.iter().enumerate() {
            let filter = Filter::new().pattern(pattern);
            let probe = make_edit_commit(&bench.repo.repo, case.head, u64::MAX - i as u64)
                .expect("probe commit");
            let filtered =
                josh_core::filter_commit(&transaction, filter, probe).expect("filter probe");
            let got = transaction.repo().find_commit(filtered).unwrap().tree_id();
            let (want, _) =
                expected_tree(transaction.repo(), probe, &glob_pred(pattern)).expect("expected");
            assert_eq!(
                got, want,
                "incremental `::{pattern}` diverged from the independent expectation"
            );
        }
    }

    let mut group = c.benchmark_group("deephistory_glob_incremental");
    // Warm iterations are ms-scale; a slightly larger sample keeps the estimate stable. No
    // throughput: the metric is time per incremental commit, not per history commit.
    group.sample_size(20);
    for &(label, pattern) in patterns {
        let filter = Filter::new().pattern(pattern);
        for case in &bench.cases {
            // Group-level warmup (once per id, NOT reset afterwards): filter the whole case
            // history so the per-iteration work is only the new tip. The cold groups above reset
            // caches per iteration, so warming must happen here regardless of group ordering.
            {
                let transaction = bench.context.open().expect("open transaction");
                josh_core::filter_commit(&transaction, filter, case.head).expect("warmup");
            }
            // Monotonic tip/counter state: every iteration advances the tip by one counter-derived
            // edit commit, so the previous iteration's timed `filter_commit` has already warmed
            // the caches for everything but the newest commit.
            let tip = std::cell::Cell::new(case.head);
            let counter = std::cell::Cell::new(0u64);
            group.bench_function(BenchmarkId::new(label, case.n_commits), |b| {
                b.iter_batched(
                    // Untimed setup: create the next edit commit on the advancing tip.
                    // PerIteration is required so setup runs immediately before each iteration
                    // and tip advancement stays sequential.
                    || {
                        let n = counter.get();
                        counter.set(n + 1);
                        let new_oid =
                            make_edit_commit(&bench.repo.repo, tip.get(), n).expect("edit commit");
                        tip.set(new_oid);
                        let transaction = bench.context.open().expect("open transaction");
                        let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                        (transaction, new_oid, iter_span)
                    },
                    // Timed: filter only the new tip; all ancestors are warm, so this measures
                    // O(1 commit) of incremental pattern-filter work.
                    |(transaction, new_oid, iter_span)| {
                        josh_core::filter_commit(&transaction, filter, new_oid)
                            .expect("filter commit");
                        (transaction, iter_span)
                    },
                    BatchSize::PerIteration,
                );
            });
        }
    }
    group.finish();
}

criterion_group!(benches, deephistory_glob);
criterion_main!(benches);
