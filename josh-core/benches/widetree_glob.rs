use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::filter::Filter;
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::path::{Path, PathBuf};

// The scaling parameter of this benchmark is tree *width*, not history length. History stays a
// fixed, short N_COMMITS while the number of files grows per case; each `filter_commit` apply of a
// `::<glob>` pattern filter (Op::Pattern) walks the whole tree in `tree::remove_pred` and
// glob-matches every blob path, so per-apply cost scales with tree size. It is the wide-tree
// counterpart of `deephistory_glob`, which holds the tree small and grows history instead.
//
// Number of files per case. Kept small in debug builds so `cargo test`/`--test` runs stay fast.
const TREE_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[100, 1_000]
} else {
    &[1_000, 10_000, 50_000]
};

// Short history: the axis is tree width, so only a handful of commits churn a few files each.
const N_COMMITS: usize = 5;
// Tree shape: three levels. File `i` lives at
// `sub_{i % N_TOP}/dir_{(i / N_TOP) % N_MID}/file_{i}.{ext}` with ext cycling rs/txt/md, so the
// recursive pattern matches ~a third of the blobs in every directory.
//
// INVARIANT: no path component ever starts with `.`. The pattern filter runs the glob crate with
// `require_literal_leading_dot`, under which `*`/`**` do NOT match dot-leading components; because
// the generated trees contain none, the plain string predicates used by the correctness gates
// below (`ends_with(".rs")`, prefix checks) are exactly equivalent to the glob semantics for THESE
// trees. A future dotfile-bearing case would need glob-faithful expectations.
const N_TOP: usize = 20;
const N_MID: usize = 10;
// Planted `sub_{k}/dir_00/config_{k}.toml` files -- the only `.toml` blobs in the tree, so the
// sparse pattern matches exactly N_SPARSE blobs regardless of tree size.
const N_SPARSE: usize = 10;
// Files edited by each of the N_COMMITS - 1 churn commits.
const CHURN_PER_COMMIT: usize = 20;

// The three benchmarked patterns: a broad recursive match, a prefix-prunable match confined to one
// top-level directory (the case a prefix-pruning specialization of `remove_pred` should speed up
// most), and a sparse recursive match that keeps a constant 10 blobs however wide the tree gets.
const PATTERN_RECURSIVE: &str = "**/*.rs";
const PATTERN_PREFIX: &str = "sub_00/**";
const PREFIX_DIR: &str = "sub_00";
const PATTERN_SPARSE: &str = "**/*.toml";

/// Expected oid of the cached bench repo's aggregate index commit. This is the cache validity key:
/// changing any build parameter above changes a case head, which changes the index commit oid,
/// which then fails the strict check in `provision_repo` and reports the new value to paste here.
/// Filled in by running the bench once after a build change. Debug builds use reduced TREE_SIZES,
/// so they have their own expected oid and their own provision-cache name (below) -- otherwise
/// `cargo test --benches` and `cargo bench` would fight over the same cache entry.
const EXPECTED_HEAD: &str = if cfg!(debug_assertions) {
    "309c42b00cb09380174a0ad6d1177d97589e8d70"
} else {
    "9b55a74d6b2d395c7e1812e615784d86740aa957"
};

/// Provision-cache name, split per profile to match the per-profile EXPECTED_HEAD.
const CACHE_NAME: &str = if cfg!(debug_assertions) {
    "widetree_glob_debug"
} else {
    "widetree_glob"
};

/// Fixed commit timestamp fed to `josh_commit_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces
/// different head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One tree width and the head of its generated history.
struct SizeCase {
    n_files: usize,
    head: git2::Oid,
}

struct GlobBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark.
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
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

        // Build (or reuse from cache) the bare repo holding every tree-width case. On a cache miss
        // the callback builds all cases (the 50k-file build runs once and is then served from the
        // provision cache), tags each tip with a `refs/heads/case_<n_files>` ref, and returns an
        // aggregate index commit whose oid is the content-addressed cache stamp checked against
        // `EXPECTED_HEAD`.
        let provisioned = josh_test_support::provision_repo::provision_repo(
            CACHE_NAME,
            &git2::Oid::from_str(EXPECTED_HEAD).expect("EXPECTED_HEAD must be a valid oid"),
            |repo| {
                let mut heads = vec![];
                for &n_files in TREE_SIZES {
                    let head = tracing::info_span!(target: "bench", "build_case", n_files)
                        .in_scope(|| build_case(repo, n_files))?;
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
            for &n_files in TREE_SIZES {
                let head = repo.refname_to_id(&format!("refs/heads/case_{n_files}"))?;
                cases.push(SizeCase { n_files, head });
            }
        }

        josh_core::cache::sled_load(provisioned.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // Correctness gates (untimed, per size case, all three patterns): confirm the filters
        // produce exactly the trees an independent git2 walk predicts, so we never silently
        // measure a filter that drops everything or is a no-op. A pattern filter keeps matching
        // blobs at their ORIGINAL paths (it does not lift subtrees to the root like `:/subdir`).
        // Run through a throwaway transaction, then reset caches so nothing here warms the timed
        // runs.
        {
            let transaction = context.open()?;
            let repo = transaction.repo();
            for case in &cases {
                // Recursive pattern: keeps exactly the `.rs` blobs everywhere (string predicate is
                // exact -- see the no-dot-component invariant above).
                let filter = Filter::new().pattern(PATTERN_RECURSIVE);
                let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
                let got = repo.find_commit(filtered)?.tree_id();
                let (want, kept) = expected_tree(repo, case.head, &|p| p.ends_with(".rs"))?;
                anyhow::ensure!(kept > 0, "recursive gate kept no blobs -- would be a no-op");
                anyhow::ensure!(
                    got == want,
                    "`::{PATTERN_RECURSIVE}` produced {got}, expected {want} (n_files {})",
                    case.n_files
                );

                // Prefix pattern, structural form: the filtered head tree must have exactly one
                // entry, `PREFIX_DIR`, whose oid equals the raw head's subtree oid. This pins down
                // "original path preserved, subtree taken wholesale -- NOT lifted to the root"
                // (valid only absent dot-components).
                let filter = Filter::new().pattern(PATTERN_PREFIX);
                let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
                let got_tree = repo.find_commit(filtered)?.tree()?;
                anyhow::ensure!(
                    got_tree.len() == 1,
                    "`::{PATTERN_PREFIX}` result must have exactly one top-level entry"
                );
                let raw_subtree = repo
                    .find_commit(case.head)?
                    .tree()?
                    .get_path(Path::new(PREFIX_DIR))?
                    .id();
                anyhow::ensure!(
                    got_tree.get_name(PREFIX_DIR).map(|e| e.id()) == Some(raw_subtree),
                    "`::{PATTERN_PREFIX}` must keep `{PREFIX_DIR}` at its original path"
                );

                // Sparse pattern: keeps exactly the N_SPARSE planted `.toml` blobs.
                let filter = Filter::new().pattern(PATTERN_SPARSE);
                let filtered = josh_core::filter_commit(&transaction, filter, case.head)?;
                let got = repo.find_commit(filtered)?.tree_id();
                let (want, kept) = expected_tree(repo, case.head, &|p| p.ends_with(".toml"))?;
                anyhow::ensure!(
                    kept == N_SPARSE,
                    "sparse gate kept {kept} blobs, expected exactly {N_SPARSE}"
                );
                anyhow::ensure!(
                    got == want,
                    "`::{PATTERN_SPARSE}` produced {got}, expected {want} (n_files {})",
                    case.n_files
                );
            }
        }
        josh_core::reset_caches()?;

        Ok(Self {
            _repo: provisioned,
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

/// Rebuild, with plain git2 tree walking plus string logic (no glob crate, no josh code), the tree
/// the pattern filter must produce: keep exactly the blobs whose full path satisfies `keep`, at
/// their ORIGINAL paths (a pattern filter preserves paths; it does not lift subtrees to the root).
/// Returns the tree oid and the number of kept blobs. The string predicates callers pass are exact
/// stand-ins for glob matching only because the generated trees contain no dot-leading path
/// components (see the invariant at the top of this file).
///
/// NOTE: a near-identical twin of this helper lives in `deephistory_glob.rs` (criterion benches
/// cannot share a module without extra plumbing; that copy's trees carry dotfiles, so its
/// predicates are glob-based and its doc text differs); keep the walker bodies in sync.
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

/// Path of file `i` in the three-level tree; the extension cycles rs/txt/md by `i % 3`.
fn file_path(i: usize) -> PathBuf {
    let ext = match i % 3 {
        0 => "rs",
        1 => "txt",
        _ => "md",
    };
    PathBuf::from(format!("sub_{:02}", i % N_TOP))
        .join(format!("dir_{:02}", (i / N_TOP) % N_MID))
        .join(format!("file_{i:06}.{ext}"))
}

/// Build a root commit holding `n_files` files in the three-level tree plus the N_SPARSE planted
/// `.toml` files, then a short churn history of N_COMMITS - 1 children each editing
/// CHURN_PER_COMMIT files at deterministic indices. The tip is tagged with
/// `refs/heads/case_<n_files>` so the head is recoverable after the repo round-trips through the
/// cache.
fn build_case(repo: &git2::Repository, n_files: usize) -> anyhow::Result<git2::Oid> {
    let mut builder = git2::build::TreeUpdateBuilder::new();
    for i in 0..n_files {
        let path = file_path(i);
        let oid = repo.blob(path.to_string_lossy().as_bytes())?;
        builder.upsert(&path, oid, git2::FileMode::Blob);
    }
    // Planted sparse files: the only `.toml` blobs, so `::**/*.toml` keeps exactly N_SPARSE blobs
    // regardless of n_files.
    for k in 0..N_SPARSE {
        let path = PathBuf::from(format!("sub_{k:02}"))
            .join("dir_00")
            .join(format!("config_{k:02}.toml"));
        let oid = repo.blob(path.to_string_lossy().as_bytes())?;
        builder.upsert(&path, oid, git2::FileMode::Blob);
    }

    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let root_tree = repo.find_tree(builder.create_updated(repo, &baseline)?)?;

    let sig = josh_commit_signature()?;
    // No ref update yet -- the tip ref is set once the history is complete.
    let mut head = repo.commit(None, &sig, &sig, "content", &root_tree, &[])?;

    // Deterministic short history: each commit churns CHURN_PER_COMMIT files at rotating indices.
    let mut rng = StdRng::seed_from_u64(1);
    for commit_idx in 0..N_COMMITS - 1 {
        let parent = repo.find_commit(head)?;
        let tree = parent.tree()?;
        let mut builder = git2::build::TreeUpdateBuilder::new();

        for j in 0..CHURN_PER_COMMIT {
            let path = file_path((commit_idx * 31 + j) % n_files);
            let content = random_string(&mut rng, 10);
            let blob = repo.blob(content.as_bytes())?;
            builder.upsert(&path, blob, git2::FileMode::Blob);
        }

        let new_tree = repo.find_tree(builder.create_updated(repo, &tree)?)?;
        head = repo.commit(
            None,
            &sig,
            &sig,
            &format!("commit {commit_idx}"),
            &new_tree,
            &[&parent],
        )?;
    }

    // Tag the tip so `setup` can find this case's head on the cache-hit path, where the build
    // callback never runs. Also keeps the whole history reachable through `git prune`.
    repo.reference(
        &format!("refs/heads/case_{n_files}"),
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

fn widetree_glob(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    // One shared setup for all three groups so the repo is provisioned only once per process.
    let bench = GlobBench::setup().expect("set up benchmark");

    let groups: &[(&str, &str)] = &[
        ("widetree_glob_recursive", PATTERN_RECURSIVE),
        ("widetree_glob_prefix", PATTERN_PREFIX),
        ("widetree_glob_sparse", PATTERN_SPARSE),
    ];

    for &(group_name, pattern) in groups {
        let filter = Filter::new().pattern(pattern);
        let mut group = c.benchmark_group(group_name);
        // The widest tree costs seconds per iteration, so keep Criterion at its minimum sample
        // count to bound the total wall-clock of a run.
        group.sample_size(10);
        for case in &bench.cases {
            group.throughput(Throughput::Elements(case.n_files as u64));
            group.bench_function(BenchmarkId::from_parameter(case.n_files), |b| {
                b.iter_batched(
                    // Per-iteration setup (untimed): start from a cold cache and a fresh
                    // transaction so every run does the full filtering work -- including the
                    // per-apply `glob::Pattern::new` compile and the full `remove_pred` walk,
                    // which are the costs under test -- instead of hitting memoized results.
                    || {
                        josh_core::reset_caches().expect("reset caches");
                        let transaction = bench.context.open().expect("open transaction");
                        let iter_span = tracing::info_span!(target: "bench", "iter").entered();
                        (transaction, iter_span)
                    },
                    // Timed: filter the case head. The setup guards are returned so they are
                    // dropped untimed after the measured section.
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
    }
}

criterion_group!(benches, widetree_glob);
criterion_main!(benches);
