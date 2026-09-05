//! Integration tests for the josh-starlark-guest interpreter blob.
//!
//! These run the built interpreter through `josh_wasm::evaluate`, replicating
//! the spec assertions of the old native josh-starlark test suite. The blob
//! is multi-MiB and not committed, so every test skips with a note on stderr
//! when it is absent — a plain `cargo test --workspace` stays green without a
//! wasm toolchain. Build it with `sh scripts/build-starlark-guest.sh`, or
//! point `JOSH_STARLARK_GUEST_WASM` at a pre-built blob.

use josh_filter::spec;

fn blob_bytes() -> Option<Vec<u8>> {
    let path = match std::env::var_os("JOSH_STARLARK_GUEST_WASM") {
        Some(p) => std::path::PathBuf::from(p),
        None => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../josh-guest/target/wasm32-unknown-unknown/release/josh_starlark_guest.wasm"),
    };
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            eprintln!(
                "skipping: starlark interpreter blob not found at {} \
                 (build with `sh scripts/build-starlark-guest.sh`)",
                path.display()
            );
            None
        }
    }
}

/// A fresh bare repository in a unique temp directory.
fn temp_repo() -> anyhow::Result<gix::Repository> {
    static DIR_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let temp_dir = std::env::temp_dir().join(format!(
        "josh_wasm_starlark_guest_test_{}_{}_{}",
        std::process::id(),
        DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(gix::init_bare(&temp_dir)?)
}

fn write_blob(repo: &gix::Repository, data: &[u8]) -> anyhow::Result<gix_hash::ObjectId> {
    Ok(repo.write_blob(data)?.detach())
}

fn entry(
    filename: &str,
    oid: gix_hash::ObjectId,
    kind: gix_object::tree::EntryKind,
) -> gix_object::tree::Entry {
    gix_object::tree::Entry {
        mode: kind.into(),
        filename: filename.into(),
        oid,
    }
}

fn write_tree(
    repo: &gix::Repository,
    mut entries: Vec<gix_object::tree::Entry>,
) -> anyhow::Result<gix_hash::ObjectId> {
    entries.sort();
    gix_object::Write::write(&repo.objects, &gix_object::Tree { entries })
        .map_err(|err| anyhow::anyhow!("write test tree: {err}"))
}

/// Evaluate `script` through the interpreter blob: the script is committed as
/// `config.star` on top of `base_tree` (the context tree the script may
/// inspect) and its path passed as the first invocation argument.
fn eval_script(
    repo: &gix::Repository,
    blob: &[u8],
    script: &str,
    base_tree: Option<gix_hash::ObjectId>,
) -> anyhow::Result<josh_filter::Filter> {
    let module_oid = write_blob(repo, blob)?;
    let script_oid = write_blob(repo, script.as_bytes())?;
    let mut entries = match base_tree {
        Some(oid) => josh_gix_ext::read_tree_entries(&repo.objects, oid)?,
        None => Vec::new(),
    };
    entries.retain(|entry| entry.filename != b"config.star");
    entries.push(entry(
        "config.star",
        script_oid,
        gix_object::tree::EntryKind::Blob,
    ));
    let tree_oid = write_tree(repo, entries)?;
    let objects = repo.path().join("objects");
    let mem = josh_memodb::MemOdb::new(None, objects.clone());
    let odb = josh_memodb::Odb::at(mem, &objects)?;
    josh_wasm::evaluate(&odb, module_oid, &["config.star".to_string()], tree_oid)
}

/// The tree the old native tree-access tests used:
/// README.md, src/{main.rs, lib/{utils.rs}}.
fn tree_fixture(repo: &gix::Repository) -> anyhow::Result<gix_hash::ObjectId> {
    let lib_tree = {
        let utils = write_blob(repo, b"pub fn helper() {}")?;
        write_tree(
            repo,
            vec![entry("utils.rs", utils, gix_object::tree::EntryKind::Blob)],
        )?
    };
    let src_tree = {
        let main = write_blob(repo, b"fn main() {}")?;
        write_tree(
            repo,
            vec![
                entry("main.rs", main, gix_object::tree::EntryKind::Blob),
                entry("lib", lib_tree, gix_object::tree::EntryKind::Tree),
            ],
        )?
    };
    let readme = write_blob(repo, b"# Project")?;
    write_tree(
        repo,
        vec![
            entry("README.md", readme, gix_object::tree::EntryKind::Blob),
            entry("src", src_tree, gix_object::tree::EntryKind::Tree),
        ],
    )
}

#[test]
fn test_simple_filter() -> anyhow::Result<()> {
    let Some(blob) = blob_bytes() else {
        return Ok(());
    };
    let repo = temp_repo()?;
    let filter = eval_script(&repo, &blob, r#"filter = filter.subdir("src")"#, None)?;
    assert_eq!(spec(filter), ":/src");
    Ok(())
}

#[test]
fn test_chain_filter() -> anyhow::Result<()> {
    let Some(blob) = blob_bytes() else {
        return Ok(());
    };
    let repo = temp_repo()?;
    let filter = eval_script(
        &repo,
        &blob,
        r#"filter = filter.subdir("src").prefix("lib")"#,
        None,
    )?;
    assert_eq!(spec(filter), ":/src:prefix=lib");
    Ok(())
}

#[test]
fn test_file_filter() -> anyhow::Result<()> {
    let Some(blob) = blob_bytes() else {
        return Ok(());
    };
    let repo = temp_repo()?;
    let filter = eval_script(&repo, &blob, r#"filter = filter.file("README.md")"#, None)?;
    // file() creates a rename from the same path to itself: ::README.md
    assert_eq!(spec(filter), "::README.md");
    Ok(())
}

#[test]
fn test_compose() -> anyhow::Result<()> {
    let Some(blob) = blob_bytes() else {
        return Ok(());
    };
    let repo = temp_repo()?;
    let script = r#"
f1 = filter.subdir("src")
f2 = filter.subdir("lib")
filter = compose([f1, f2])
"#;
    let filter = eval_script(&repo, &blob, script, None)?;
    assert_eq!(spec(filter), ":[:/src,:/lib]");
    Ok(())
}

#[test]
fn test_tree_build_filter_from_all_files() -> anyhow::Result<()> {
    let Some(blob) = blob_bytes() else {
        return Ok(());
    };
    let repo = temp_repo()?;
    let base = tree_fixture(&repo)?;
    // Unlike the old native test, the script itself is part of the tree it
    // walks (it must be in the context to be runnable), so it skips .star
    // files to reproduce the historical expected spec.
    let script = r#"
def collect_all_files(dir_path=""):
    """Recursively collect all files and build filters"""
    filters = []
    for file_path in tree.files(dir_path):
        if not file_path.endswith(".star"):
            filters.append(filter.file(file_path))
    for subdir_path in tree.dirs(dir_path):
        filters.extend(collect_all_files(subdir_path))
    return filters

all_file_filters = collect_all_files("")
filter = compose(all_file_filters)
"#;
    let filter = eval_script(&repo, &blob, script, Some(base))?;
    // File filters with multi-component paths decompose into Subdir + File +
    // Prefix chains; common Subdir/Prefix layers get factored out.
    assert_eq!(
        spec(filter),
        ":[::README.md,:/src:[:/lib::utils.rs:prefix=lib,::main.rs]:prefix=src]"
    );
    Ok(())
}

#[test]
fn test_determinism_across_cache_clear() -> anyhow::Result<()> {
    let Some(blob) = blob_bytes() else {
        return Ok(());
    };
    let repo = temp_repo()?;
    // Iterates a dynamically built dict to smoke iteration-order stability in
    // the interpreter internals (wasm has no entropy imports, so hash seeds
    // are fixed; this guards against order nondeterminism regardless). The
    // loops live inside a def: Dialect::Standard forbids top-level `for`.
    let script = r#"
def build():
    d = {}
    for i in range(20):
        d["k" + str(i)] = "sub" + str(i)
    parts = []
    for k in d:
        parts.append(filter.subdir(d[k]).prefix(k))
    for k in sorted(d.keys(), reverse = True):
        parts.append(filter.subdir(d[k]))
    return parts

filter = compose(build())
"#;
    let first = eval_script(&repo, &blob, script, None)?;
    let first_spec = spec(first);
    josh_wasm::clear_caches();
    let second = eval_script(&repo, &blob, script, None)?;
    // Filters are interned: equality is node identity, so an identical spec
    // AND an identical handle prove the two evaluations produced the same
    // filter even though the second one fully re-ran (caches were cleared).
    assert_eq!(first, second);
    assert_eq!(first_spec, spec(second));
    Ok(())
}
