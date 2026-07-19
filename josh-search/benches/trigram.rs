use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use josh_core::git::josh_commit_signature;
use rand::prelude::*;
use std::path::PathBuf;

// Benchmarks for the trigram indexer (`josh_search`), covering its three
// externally meaningful operations through the public API only, so the numbers stay comparable
// across an internal rework of the indexer:
//
//   * `trigram_index_full`        -- cold-cache indexing of a whole tree
//   * `trigram_index_incremental` -- indexing churn commits while the parent's index is warm
//   * `trigram_search`            -- `search_candidates` + `search_matches`, the end-to-end
//                                    user-visible search operation (see `josh-filter --search`)
//
// The scaling parameter is the number of files in the tree; history stays short (a root commit
// plus `K_CHURN` small churn commits, used only by the incremental group).

// Number of files per case. Kept small in debug builds so `cargo test --benches` runs stay fast.
const CASE_SIZES: &[usize] = if cfg!(debug_assertions) {
    &[30, 120]
} else {
    &[100, 1_000, 10_000]
};

// Tree layout: file `i` lives at `top_XX/sub_YY/file_IIIII` with LEAF_FILES files per leaf
// directory and LEAVES_PER_TOP leaf directories per top-level directory. Two nesting levels so
// the indexer's per-directory recursion and OWN/SUB aggregation are actually exercised.
const LEAF_FILES: usize = 25;
const LEAVES_PER_TOP: usize = 16;

// File content: WORDS_PER_FILE words (8 per line, ~2 KB per file) sampled with a skewed
// distribution from a fixed vocabulary of VOCAB_SIZE lowercase words. The bounded vocabulary
// (originally required by the bloom indexer's saturation ceiling) is kept unchanged so results
// stay comparable with the pre-rework baseline; ~1-2k distinct trigrams per file is also a
// realistic density for source code. The rare-needle candidate gate in `setup` is the tripwire
// if these constants degenerate.
const WORDS_PER_FILE: usize = 300;
const WORDS_PER_LINE: usize = 8;
const VOCAB_SIZE: usize = 512;

// Churn history for the incremental group: K_CHURN commits on top of the root, each regenerating
// ~CHURN_FRACTION of the files (at least one).
const K_CHURN: usize = if cfg!(debug_assertions) { 3 } else { 20 };
const CHURN_FRACTION: f64 = 0.01;

// Needles for the search group, planted keyed on file index (not revision) so churn commits
// preserve them: NEEDLE_RARE in exactly one file, NEEDLE_COMMON in every COMMON_EVERY-th file,
// NEEDLE_ABSENT nowhere. Nonsense compounds so they cannot occur in the generated text.
const NEEDLE_RARE: &str = "xylophonequagmirezephyr";
const NEEDLE_COMMON: &str = "quixoticjubileewombat";
const NEEDLE_ABSENT: &str = "grumblesnorkelvortex";
const COMMON_EVERY: usize = 20;

/// Expected oid of the cached bench repo's aggregate index commit. This is the cache validity
/// key: changing any build parameter above changes a case head, which changes the index commit
/// oid, which then fails the strict check in `provision_repo` and reports the new value to paste
/// here. Filled in by running the bench once after a build change. Debug and release builds use
/// different case sizes, hence different stamps (and different cache names, see `TESTCASE`).
const EXPECTED_HEAD: &str = if cfg!(debug_assertions) {
    "064c638fed3f20f9171e521dce0ae4030db30ff4"
} else {
    "1714feee821f990b51b713cf2ca632c9475274cb"
};

/// Cache name under the user cache dir, per profile: debug and release build different repos
/// (different CASE_SIZES/K_CHURN), so they must not evict each other's cached build.
const TESTCASE: &str = if cfg!(debug_assertions) {
    "trigram_debug"
} else {
    "trigram"
};

/// Fixed commit timestamp fed to `josh_commit_signature()` via `JOSH_COMMIT_TIME` so the built
/// history is reproducible. Without it the signature uses the wall clock, every run produces
/// different head oids, and `EXPECTED_HEAD` can never be stable. The value itself is arbitrary.
const JOSH_BENCH_COMMIT_TIME: &str = "1700000000";

/// One case size with everything the bench groups need: the commit chain (root first, tip last),
/// the pre-built (and flushed) index tree of the tip, and throughput/gate bookkeeping.
struct Case {
    n_files: usize,
    chain: Vec<git2::Oid>,
    index_tree_oid: git2::Oid,
    total_bytes: u64,
}

impl Case {
    fn tip(&self) -> git2::Oid {
        *self.chain.last().expect("chain is never empty")
    }
}

struct TrigramBench {
    // Keeps the on-disk repository (and its tempdir) alive for the duration of the benchmark.
    _repo: josh_test_support::provision_repo::ProvisionedRepo,
    // A fresh transaction is opened from this for every iteration.
    context: josh_core::cache::TransactionContext,
    cases: Vec<Case>,
}

fn path_for(i: usize) -> PathBuf {
    PathBuf::from(format!("top_{:02}", i / (LEAF_FILES * LEAVES_PER_TOP)))
        .join(format!("sub_{:02}", (i / LEAF_FILES) % LEAVES_PER_TOP))
        .join(format!("file_{i:05}"))
}

fn random_word(rng: &mut StdRng) -> String {
    let len = rng.random_range(3..=10);
    (0..len)
        .map(|_| {
            use rand::distr::Alphabetic;
            let ch = Alphabetic.sample(rng) as char;
            ch.to_ascii_lowercase()
        })
        .collect()
}

/// The fixed vocabulary all file content is sampled from.
fn vocabulary() -> Vec<String> {
    let mut rng = StdRng::seed_from_u64(0);
    (0..VOCAB_SIZE).map(|_| random_word(&mut rng)).collect()
}

/// Deterministic content of file `i` at churn revision `revision`. Needle lines are keyed on the
/// file index only, so regenerating a file in a churn commit preserves its needles.
fn file_content(vocab: &[String], n_files: usize, i: usize, revision: usize) -> String {
    let mut rng = StdRng::seed_from_u64(((i as u64) << 8) | revision as u64);

    // Needles go at the very start of the file. Historical: the bloom indexer emitted per-file
    // filters in chunks and missed needles whose trigrams straddled a chunk boundary, so the
    // fixture planted them in the first chunk. The exact index has no such failure mode, but the
    // placement is kept so the content stays byte-identical to the pre-rework baseline.
    let mut lines = vec![];
    if i == n_files / 2 {
        lines.push(format!("marker {} end", NEEDLE_RARE));
    }
    if i % COMMON_EVERY == 0 {
        lines.push(format!("marker {} end", NEEDLE_COMMON));
    }
    for _ in 0..WORDS_PER_FILE / WORDS_PER_LINE {
        let line = (0..WORDS_PER_LINE)
            .map(|_| {
                // Squaring a uniform sample skews word choice toward the front of the
                // vocabulary, giving a more realistic (non-uniform) trigram distribution.
                let u: f64 = rng.random();
                let idx = ((u * u) * VOCAB_SIZE as f64) as usize;
                vocab[idx.min(VOCAB_SIZE - 1)].as_str()
            })
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(line);
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Build one case: a root commit holding all `n_files` files at revision 0, then `K_CHURN` churn
/// commits each regenerating ~CHURN_FRACTION of the files (at least one) at a new revision. The
/// tip is tagged `refs/heads/case_<n_files>` so the chain is recoverable after the repo
/// round-trips through the provision cache.
fn build_case(
    repo: &git2::Repository,
    vocab: &[String],
    n_files: usize,
) -> anyhow::Result<git2::Oid> {
    let mut builder = git2::build::TreeUpdateBuilder::new();
    for i in 0..n_files {
        let oid = repo.blob(file_content(vocab, n_files, i, 0).as_bytes())?;
        builder.upsert(&path_for(i), oid, git2::FileMode::Blob);
    }

    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let root_tree = repo.find_tree(builder.create_updated(repo, &baseline)?)?;

    let sig = josh_commit_signature()?;
    // No ref update yet -- the tip ref is set once the history is complete.
    let mut head = repo.commit(None, &sig, &sig, "content", &root_tree, &[])?;

    let n_churn_files = std::cmp::max(1, (n_files as f64 * CHURN_FRACTION) as usize);
    let mut rng = StdRng::seed_from_u64(n_files as u64);
    for revision in 1..=K_CHURN {
        let parent = repo.find_commit(head)?;
        let tree = parent.tree()?;
        let mut builder = git2::build::TreeUpdateBuilder::new();

        // Dedup: TreeUpdateBuilder rejects duplicate entries for the same path.
        let churned: std::collections::BTreeSet<usize> = (0..n_churn_files)
            .map(|_| rng.random_range(0..n_files))
            .collect();
        for i in churned {
            let oid = repo.blob(file_content(vocab, n_files, i, revision).as_bytes())?;
            builder.upsert(&path_for(i), oid, git2::FileMode::Blob);
        }

        let new_tree = repo.find_tree(builder.create_updated(repo, &tree)?)?;
        head = repo.commit(
            None,
            &sig,
            &sig,
            &format!("churn {revision}"),
            &new_tree,
            &[&parent],
        )?;
    }

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
/// all cases reachable so provision_repo's `git prune` retains the full history.
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

/// Sum of all blob sizes in `tree`, the throughput denominator for the indexing groups.
fn tree_content_bytes(repo: &git2::Repository, tree: &git2::Tree) -> anyhow::Result<u64> {
    let mut total = 0u64;
    tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            if let Ok(blob) = repo.find_blob(entry.id()) {
                total += blob.size() as u64;
            }
        }
        git2::TreeWalkResult::Ok
    })?;
    Ok(total)
}

/// Recover the first-parent chain (root first, tip last) of a case from its tip ref.
fn recover_chain(repo: &git2::Repository, n_files: usize) -> anyhow::Result<Vec<git2::Oid>> {
    let mut chain = vec![];
    let mut oid = repo.refname_to_id(&format!("refs/heads/case_{n_files}"))?;
    loop {
        chain.push(oid);
        match repo.find_commit(oid)?.parent_id(0) {
            Ok(parent) => oid = parent,
            Err(_) => break,
        }
    }
    chain.reverse();
    anyhow::ensure!(
        chain.len() == K_CHURN + 1,
        "expected root + {K_CHURN} churn commits, found {}",
        chain.len()
    );
    Ok(chain)
}

/// End-to-end search exactly as `josh-filter --search` performs it: candidate selection over
/// the index tree, then exact matching over the source tree.
fn search(
    repo: &git2::Repository,
    index_tree: &git2::Tree,
    source_tree: &git2::Tree,
    needle: &str,
) -> anyhow::Result<(Vec<String>, Vec<(String, Vec<(usize, String)>)>)> {
    let candidates = josh_search::search_candidates(repo, index_tree, source_tree, needle)?;
    let matches = josh_search::search_matches(repo, source_tree, needle, &candidates)?;
    Ok((candidates, matches))
}

impl TrigramBench {
    fn setup() -> anyhow::Result<Self> {
        let _setup = tracing::info_span!(target: "bench", "setup").entered();

        // Pin commit timestamps before building so the head oids are reproducible and
        // `EXPECTED_HEAD` stays valid across runs. Must run before the cache-miss path invokes
        // the build callback.
        // SAFETY: setup runs single-threaded, before any benchmark iteration.
        unsafe {
            std::env::set_var("JOSH_COMMIT_TIME", JOSH_BENCH_COMMIT_TIME);
        }

        let provisioned = josh_test_support::provision_repo::provision_repo(
            TESTCASE,
            &git2::Oid::from_str(EXPECTED_HEAD).expect("EXPECTED_HEAD must be a valid oid"),
            |repo| {
                let vocab = vocabulary();
                let mut heads = vec![];
                for &n_files in CASE_SIZES {
                    let head = tracing::info_span!(target: "bench", "build_case", n_files)
                        .in_scope(|| build_case(repo, &vocab, n_files))?;
                    heads.push(head);
                }
                build_index(repo, &heads)
            },
        )?;

        josh_core::cache::sled_load(provisioned.path())?;
        let cache = std::sync::Arc::new(
            josh_core::cache::CacheStack::new()
                .with_backend(josh_core::cache::SledCacheBackend::default()),
        );
        let context = josh_core::cache::TransactionContext::new(provisioned.path(), cache);

        // Per case (untimed): pre-build the tip's index tree for the search group and run the
        // correctness gates, so we never silently bench an indexer or search that stopped
        // finding anything (`get_blob` returns "" for missing entries, so breakage here is
        // silent by default).
        let mut cases = vec![];
        for &n_files in CASE_SIZES {
            let chain = recover_chain(&provisioned.repo, n_files)?;
            let tip = *chain.last().unwrap();

            josh_core::reset_caches()?;
            let transaction = context.open()?;
            let repo = transaction.repo();
            let tip_tree = repo.find_commit(tip)?.tree()?;
            let total_bytes = tree_content_bytes(repo, &tip_tree)?;

            // Cold-build the tip index, then flush: `trigram_search` opens fresh repository
            // handles per directory, and those cannot see objects still sitting in this
            // transaction's in-memory odb.
            let index_tree = josh_search::trigram_index(repo, &transaction, tip_tree.clone())?;
            let index_tree_oid = index_tree.id();
            transaction.flush_mem_odb()?;

            // Gate: the rare needle is found in exactly its planted file, with a tight
            // candidate set. A blown-up candidate list means candidate selection has
            // degenerated and the search numbers would be meaningless. (The bound is kept at
            // the pre-rework value of 5; the exact index should always produce exactly 1.)
            let rare_path = path_for(n_files / 2).to_string_lossy().into_owned();
            let (candidates, matches) = search(repo, &index_tree, &tip_tree, NEEDLE_RARE)?;
            anyhow::ensure!(
                matches.len() == 1 && matches[0].0 == rare_path,
                "rare needle not found in exactly its planted file {rare_path}: {matches:?}"
            );
            anyhow::ensure!(
                candidates.len() <= 5,
                "rare needle produced {} candidates -- candidate selection degenerated",
                candidates.len()
            );

            // Gate: the common needle is found in every planted file, the absent one nowhere.
            let common_count = n_files.div_ceil(COMMON_EVERY);
            let (_, matches) = search(repo, &index_tree, &tip_tree, NEEDLE_COMMON)?;
            anyhow::ensure!(
                matches.len() == common_count,
                "common needle found in {} files, expected {common_count}",
                matches.len()
            );
            let (_, matches) = search(repo, &index_tree, &tip_tree, NEEDLE_ABSENT)?;
            anyhow::ensure!(
                matches.is_empty(),
                "absent needle found in {} files",
                matches.len()
            );

            // Gate: the incremental path (root warm, then indexing every churn commit) must
            // produce the exact same tip index as the cold build above, and churn must actually
            // have changed the index. This is the property the planned rework must preserve.
            josh_core::reset_caches()?;
            let transaction = context.open()?;
            let repo = transaction.repo();
            let root_tree = repo.find_commit(chain[0])?.tree()?;
            let root_index_oid = josh_search::trigram_index(repo, &transaction, root_tree)?.id();
            let mut incremental_oid = root_index_oid;
            for &oid in &chain[1..] {
                let tree = repo.find_commit(oid)?.tree()?;
                incremental_oid = josh_search::trigram_index(repo, &transaction, tree)?.id();
            }
            anyhow::ensure!(
                incremental_oid == index_tree_oid,
                "incremental tip index {incremental_oid} != cold tip index {index_tree_oid}"
            );
            anyhow::ensure!(
                incremental_oid != root_index_oid,
                "churn commits did not change the index"
            );

            cases.push(Case {
                n_files,
                chain,
                index_tree_oid,
                total_bytes,
            });
        }

        // Nothing from setup may warm the timed groups. The pre-built index trees survive this:
        // they live in the repo's git objects (flushed above), not in the sled cache.
        josh_core::reset_caches()?;

        Ok(Self {
            _repo: provisioned,
            context,
            cases,
        })
    }
}

fn trigram_benches(c: &mut Criterion) {
    josh_test_support::init_tracing("bench=trace");

    let bench = TrigramBench::setup().expect("set up benchmark");

    // Cold-cache indexing of the whole tip tree. Throughput is content bytes: the indexer's work
    // is per-byte, the file count is in the benchmark id. The transaction drop (mem-odb flush)
    // stays untimed, consistent with the josh-core benches.
    let mut group = c.benchmark_group("trigram_index_full");
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Bytes(case.total_bytes));
        group.bench_function(BenchmarkId::from_parameter(case.n_files), |b| {
            b.iter_with_setup_wrapper(|runner| {
                josh_core::reset_caches().expect("reset caches");
                let transaction = bench.context.open().expect("open transaction");
                let repo = transaction.repo();
                let tip_tree = repo
                    .find_commit(case.tip())
                    .expect("find tip")
                    .tree()
                    .expect("tip tree");

                runner.run(|| {
                    josh_search::trigram_index(repo, &transaction, tip_tree.clone())
                        .expect("index tree")
                });
            });
        });
    }
    group.finish();

    // Per-commit incremental indexing: warm the root commit's index (untimed), then index every
    // churn commit in order -- each only recomputes the directories the churn touched. A chain of
    // K_CHURN commits rather than a single child amortizes noise and matches the real "index
    // every new commit" usage; Throughput::Elements reports time per churn commit. The full
    // untimed re-warm per iteration is unavoidable: the sled cache reset is all-or-nothing.
    let mut group = c.benchmark_group("trigram_index_incremental");
    group.sample_size(10);
    for case in &bench.cases {
        group.throughput(Throughput::Elements(K_CHURN as u64));
        group.bench_function(BenchmarkId::from_parameter(case.n_files), |b| {
            b.iter_with_setup_wrapper(|runner| {
                josh_core::reset_caches().expect("reset caches");
                let transaction = bench.context.open().expect("open transaction");
                let repo = transaction.repo();
                let root_tree = repo
                    .find_commit(case.chain[0])
                    .expect("find root")
                    .tree()
                    .expect("root tree");
                josh_search::trigram_index(repo, &transaction, root_tree).expect("warm root index");

                runner.run(|| {
                    for &oid in &case.chain[1..] {
                        let tree = repo
                            .find_commit(oid)
                            .expect("find churn commit")
                            .tree()
                            .expect("churn tree");
                        josh_search::trigram_index(repo, &transaction, tree)
                            .expect("incremental index");
                    }
                });
            });
        });
    }
    group.finish();

    // End-to-end search over the pre-built index: candidate selection plus exact matching, for a
    // needle in one file, in every COMMON_EVERY-th file, and in none. Search has no result
    // memoization and reads only git objects, so the cache reset is for uniformity with the
    // other groups, not correctness.
    let mut group = c.benchmark_group("trigram_search");
    group.sample_size(10);
    for case in &bench.cases {
        for (kind, needle) in [
            ("rare", NEEDLE_RARE),
            ("common", NEEDLE_COMMON),
            ("absent", NEEDLE_ABSENT),
        ] {
            group.throughput(Throughput::Elements(case.n_files as u64));
            group.bench_function(BenchmarkId::new(kind, case.n_files), |b| {
                b.iter_with_setup_wrapper(|runner| {
                    josh_core::reset_caches().expect("reset caches");
                    let transaction = bench.context.open().expect("open transaction");
                    let repo = transaction.repo();
                    let index_tree = repo
                        .find_tree(case.index_tree_oid)
                        .expect("find prebuilt index tree");
                    let source_tree = repo
                        .find_commit(case.tip())
                        .expect("find tip")
                        .tree()
                        .expect("tip tree");

                    runner.run(|| search(repo, &index_tree, &source_tree, needle).expect("search"));
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, trigram_benches);
criterion_main!(benches);
