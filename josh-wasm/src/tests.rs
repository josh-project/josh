use crate::cache;
use crate::engine::{Engine, EvalContext};
use josh_filter::spec;

/// Serializes tests that assert on or clear the global caches; without this a
/// parallel `clear_caches` drops another test's memo entries mid-assertion.
static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cache_lock() -> std::sync::MutexGuard<'static, ()> {
    CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fresh bare repository and its transaction-compatible object facade.
struct TestRepo {
    repo: gix::Repository,
    odb: josh_memodb::Odb,
}

impl std::ops::Deref for TestRepo {
    type Target = gix::Repository;

    fn deref(&self) -> &Self::Target {
        &self.repo
    }
}

impl TestRepo {
    fn odb(&self) -> &josh_memodb::Odb {
        &self.odb
    }
}

fn temp_repo() -> anyhow::Result<TestRepo> {
    static DIR_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let temp_dir = std::env::temp_dir().join(format!(
        "josh_wasm_test_{}_{}_{}",
        std::process::id(),
        DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = gix::init_bare(&temp_dir)?;
    let objects = repo.path().join("objects");
    let mem = josh_memodb::MemOdb::new(None, objects.clone());
    let odb = josh_memodb::Odb::at(mem, &objects)?;
    Ok(TestRepo { repo, odb })
}

fn write_blob(repo: &gix::Repository, data: &[u8]) -> anyhow::Result<gix_hash::ObjectId> {
    Ok(repo.write_blob(data)?.detach())
}

fn write_tree(
    repo: &gix::Repository,
    mut entries: Vec<gix_object::tree::Entry>,
) -> anyhow::Result<gix_hash::ObjectId> {
    entries.sort();
    gix_object::Write::write(&repo.objects, &gix_object::Tree { entries })
        .map_err(|err| anyhow::anyhow!("write test tree: {err}"))
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

fn empty_tree(repo: &gix::Repository) -> anyhow::Result<gix_hash::ObjectId> {
    write_tree(repo, vec![])
}

fn eval(
    repo: &TestRepo,
    module: &str,
    args: &[&str],
    tree_oid: gix_hash::ObjectId,
) -> anyhow::Result<josh_filter::Filter> {
    let module_oid = write_blob(repo, module.as_bytes())?;
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    crate::evaluate(repo.odb(), module_oid, &args, tree_oid)
}

/// Standard module skeleton: ABI exports plus a bump `josh_alloc`, with the
/// given imports, data segments and `josh_run` body spliced in.
fn module(imports: &str, data: &str, body: &str) -> String {
    format!(
        r#"(module
  {imports}
  (memory (export "memory") 1)
  {data}
  (global $next (mut i32) (i32.const 4096))
  (func (export "josh_abi_version") (result i32) (i32.const 1))
  (func (export "josh_alloc") (param i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get 0)))
    (local.get $ptr))
  (func (export "josh_run") (result i32)
    {body}))"#
    )
}

/// Probe a `(path) -> string` host import: the guest calls it with `path` and
/// wraps the returned string in `with_meta(nop(), "r", <string>)`, which the
/// test reads back via `Filter::get_meta` (newline-safe, unlike specs).
fn probe_string(
    repo: &TestRepo,
    tree_oid: gix_hash::ObjectId,
    import: &str,
    path: &str,
    args: &[&str],
) -> anyhow::Result<Option<String>> {
    let path_len = path.len();
    let (probe_import, probe_call) = if import == "invocation_args" {
        (
            format!(r#"(import "josh" "{import}" (func $probe (result i64)))"#),
            "(call $probe)".to_string(),
        )
    } else {
        (
            format!(r#"(import "josh" "{import}" (func $probe (param i32 i32) (result i64)))"#),
            format!("(call $probe (i32.const 32) (i32.const {path_len}))"),
        )
    };
    let wat = module(
        &format!(
            r#"(import "josh" "nop" (func $nop (result i32)))
  (import "josh" "with_meta" (func $with_meta (param i32 i32 i32 i32 i32) (result i32)))
  {probe_import}"#
        ),
        &format!(
            r#"(data (i32.const 16) "r")
  (data (i32.const 32) "{path}")"#
        ),
        &format!(
            r#"(local $s i64)
    (local.set $s {probe_call})
    (call $with_meta (call $nop)
      (i32.const 16) (i32.const 1)
      (i32.wrap_i64 (i64.shr_u (local.get $s) (i64.const 32)))
      (i32.wrap_i64 (local.get $s)))"#
        ),
    );
    Ok(eval(repo, &wat, args, tree_oid)?.get_meta("r"))
}

/// Tree used by the tree-access tests:
/// README.md, hello.txt ("sub1"), bin.dat (binary), odd.txt (non-UTF-8),
/// src/{main.rs, lib/{utils.rs}}.
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
    let hello = write_blob(repo, b"sub1")?;
    let binary = write_blob(repo, &[0u8, 159, 146, 150])?;
    let non_utf8 = write_blob(repo, &[0xff, 0xfe, 0xfd])?;
    write_tree(
        repo,
        vec![
            entry("README.md", readme, gix_object::tree::EntryKind::Blob),
            entry("hello.txt", hello, gix_object::tree::EntryKind::Blob),
            entry("bin.dat", binary, gix_object::tree::EntryKind::Blob),
            entry("odd.txt", non_utf8, gix_object::tree::EntryKind::Blob),
            entry("src", src_tree, gix_object::tree::EntryKind::Tree),
        ],
    )
}

#[test]
fn test_nop_module() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))"#,
        "",
        "(call $nop)",
    );
    let filter = eval(&repo, &wat, &[], tree)?;
    assert!(filter.is_nop());
    Ok(())
}

#[test]
fn test_subdir_from_data_section() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = r#"(module
  (import "josh" "nop" (func $nop (result i32)))
  (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "sub1")
  (func (export "josh_abi_version") (result i32) (i32.const 1))
  (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "josh_run") (result i32)
    (call $subdir (call $nop) (i32.const 16) (i32.const 4))))"#;
    let filter = eval(&repo, wat, &[], tree)?;
    assert_eq!(spec(filter), ":/sub1");
    Ok(())
}

#[test]
fn test_chain_and_prefix() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))
  (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  (import "josh" "prefix" (func $prefix (param i32 i32 i32) (result i32)))
  (import "josh" "chain" (func $chain (param i32 i32) (result i32)))"#,
        r#"(data (i32.const 16) "src") (data (i32.const 24) "lib")"#,
        r#"(call $chain
      (call $subdir (call $nop) (i32.const 16) (i32.const 3))
      (call $prefix (call $nop) (i32.const 24) (i32.const 3)))"#,
    );
    let filter = eval(&repo, &wat, &[], tree)?;
    assert_eq!(spec(filter), ":/src:prefix=lib");
    Ok(())
}

#[test]
fn test_compose() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))
  (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  (import "josh" "compose" (func $compose (param i32 i32) (result i32)))"#,
        r#"(data (i32.const 16) "ab")"#,
        r#"(i32.store (i32.const 64) (call $subdir (call $nop) (i32.const 16) (i32.const 1)))
    (i32.store (i32.const 68) (call $subdir (call $nop) (i32.const 17) (i32.const 1)))
    (call $compose (i32.const 64) (i32.const 2))"#,
    );
    let filter = eval(&repo, &wat, &[], tree)?;
    assert_eq!(spec(filter), ":[:/a,:/b]");
    Ok(())
}

#[test]
fn test_rename_dst_first() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))
  (import "josh" "rename" (func $rename (param i32 i32 i32 i32 i32) (result i32)))"#,
        r#"(data (i32.const 16) "dstsrc")"#,
        r#"(call $rename (call $nop) (i32.const 16) (i32.const 3) (i32.const 19) (i32.const 3))"#,
    );
    let filter = eval(&repo, &wat, &[], tree)?;
    // Destination comes first in the builder and in the spec.
    assert_eq!(spec(filter), "::dst=src");
    Ok(())
}

#[test]
fn test_tree_file() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = tree_fixture(&repo)?;
    assert_eq!(
        probe_string(&repo, tree, "tree_file", "hello.txt", &[])?,
        Some("sub1".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_file", "missing.txt", &[])?,
        Some("".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_file", "bin.dat", &[])?,
        Some("".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_file", "odd.txt", &[])?,
        Some("".to_string())
    );
    // A tree entry is not a blob.
    assert_eq!(
        probe_string(&repo, tree, "tree_file", "src", &[])?,
        Some("".to_string())
    );
    Ok(())
}

#[test]
fn test_tree_files_and_dirs() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = tree_fixture(&repo)?;
    // Immediate child blobs only, full paths, git tree entry order.
    assert_eq!(
        probe_string(&repo, tree, "tree_files", "", &[])?,
        Some("README.md\nbin.dat\nhello.txt\nodd.txt".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_files", "src", &[])?,
        Some("src/main.rs".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_dirs", "", &[])?,
        Some("src".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_dirs", "src", &[])?,
        Some("src/lib".to_string())
    );
    // Bad paths yield empty results.
    assert_eq!(
        probe_string(&repo, tree, "tree_files", "nope", &[])?,
        Some("".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_dirs", "hello.txt", &[])?,
        Some("".to_string())
    );
    Ok(())
}

#[test]
fn test_tree_entry_oid() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = tree_fixture(&repo)?;
    let expected = write_blob(&repo, b"sub1")?.to_string();
    assert_eq!(
        probe_string(&repo, tree, "tree_entry_oid", "hello.txt", &[])?,
        Some(expected)
    );
    assert_eq!(
        probe_string(&repo, tree, "tree_entry_oid", "missing.txt", &[])?,
        Some("".to_string())
    );
    // The empty path names the context tree itself.
    assert_eq!(
        probe_string(&repo, tree, "tree_entry_oid", "", &[])?,
        Some(tree.to_string())
    );
    Ok(())
}

#[test]
fn test_invocation_args() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    assert_eq!(
        probe_string(&repo, tree, "invocation_args", "", &[])?,
        Some("".to_string())
    );
    assert_eq!(
        probe_string(&repo, tree, "invocation_args", "", &["a", "b"])?,
        Some("a\nb".to_string())
    );
    Ok(())
}

#[test]
fn test_invalid_handle_traps() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))"#,
        r#"(data (i32.const 16) "x")"#,
        r#"(call $subdir (i32.const 99) (i32.const 16) (i32.const 1))"#,
    );
    let result = eval(&repo, &wat, &[], tree);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("invalid filter handle")
    );
    Ok(())
}

#[test]
fn test_bad_abi_version() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = r#"(module
  (memory (export "memory") 1)
  (func (export "josh_abi_version") (result i32) (i32.const 2))
  (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "josh_run") (result i32) (i32.const 0)))"#;
    let result = eval(&repo, wat, &[], tree);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ABI version"));
    Ok(())
}

#[test]
fn test_out_of_fuel() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module("", "", r#"(loop $l (br $l)) (i32.const 0)"#);
    let result = eval(&repo, &wat, &[], tree);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("fuel")
    );
    Ok(())
}

#[test]
fn test_memory_limit() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    // 4096 pages = 256 MiB, over the default 64 MiB limit.
    let wat = module(
        "",
        "",
        r#"(drop (memory.grow (i32.const 4096))) (i32.const 0)"#,
    );
    let result = eval(&repo, &wat, &[], tree);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_table_allocation_limit() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    // A tiny module declaring a huge table minimum: wasmi commits the declared
    // minimum eagerly at instantiation (before any fuel is consumed), so the
    // store limits must reject it -- the memory limit alone does not apply to
    // tables.
    let wat = r#"(module
  (table 50000000 funcref)
  (memory (export "memory") 1)
  (func (export "josh_abi_version") (result i32) (i32.const 1))
  (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "josh_run") (result i32) (i32.const 0)))"#;
    let result = eval(&repo, wat, &[], tree);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_multiple_memories_rejected() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    // wasmi's default config enables multi-memory; the store limits cap the
    // instance at one memory so per-memory limits cannot be stacked.
    let wat = r#"(module
  (memory (export "memory") 1)
  (memory 1)
  (func (export "josh_abi_version") (result i32) (i32.const 1))
  (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "josh_run") (result i32) (i32.const 0)))"#;
    let result = eval(&repo, wat, &[], tree);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_handle_limit() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    // One handle over the limit: constructed filters are interned for the
    // process lifetime, so unbounded construction must trap.
    let count = *crate::JOSH_WASM_MAX_HANDLES + 1;
    let body = format!(
        r#"(local $i i32)
    (block $done
      (loop $l
        (br_if $done (i32.ge_u (local.get $i) (i32.const {count})))
        (drop (call $nop))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $l)))
    (i32.const 0)"#
    );
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))"#,
        "",
        &body,
    );
    let result = eval(&repo, &wat, &[], tree);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("filter handle limit")
    );
    Ok(())
}

#[test]
fn test_tree_file_size_cap() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let content = "a".repeat(100);
    let blob = write_blob(&repo, content.as_bytes())?;
    let tree = write_tree(
        &repo,
        vec![entry("big.txt", blob, gix_object::tree::EntryKind::Blob)],
    )?;
    // Oversized blobs are never materialized host-side; within the cap the
    // content is returned as usual.
    assert_eq!(
        crate::host::tree_file_content(repo.odb(), tree, "big.txt", 10),
        ""
    );
    assert_eq!(
        crate::host::tree_file_content(repo.odb(), tree, "big.txt", 100),
        content
    );
    Ok(())
}

#[test]
fn test_module_size_limit() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let oversized = vec![0u8; *crate::JOSH_WASM_MODULE_SIZE_LIMIT + 1];
    let module_oid = write_blob(&repo, &oversized)?;
    let result = crate::evaluate(repo.odb(), module_oid, &[], tree);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("size limit"));
    Ok(())
}

#[test]
fn test_binary_wasm_and_garbage_text() -> anyhow::Result<()> {
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    // A binary module (with the \0asm magic) runs without WAT assembly.
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))"#,
        "",
        "(call $nop)",
    );
    let binary = wat::parse_str(&wat)?;
    assert!(binary.starts_with(b"\0asm"));
    let module_oid = write_blob(&repo, &binary)?;
    let filter = crate::evaluate(repo.odb(), module_oid, &[], tree)?;
    assert!(filter.is_nop());
    // Garbage text is neither a binary nor valid WAT.
    let result = eval(&repo, "this is not a wasm module", &[], tree);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_memoization() -> anyhow::Result<()> {
    let _guard = cache_lock();
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    // Unique data string so this test's module OID is not shared with any
    // other test (the caches are global and tests run in parallel).
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))"#,
        r#"(data (i32.const 900) "memoization-test-unique")"#,
        "(call $nop)",
    );
    let module_oid = write_blob(&repo, wat.as_bytes())?;
    let f1 = crate::evaluate(repo.odb(), module_oid, &[], tree)?;
    let f2 = crate::evaluate(repo.odb(), module_oid, &[], tree)?;
    // Filter equality is interned-pointer equality.
    assert!(f1 == f2);
    assert_eq!(cache::eval_memo_entries_for(module_oid), 1);
    // Different args are a different memo key.
    let _ = crate::evaluate(repo.odb(), module_oid, &["x".to_string()], tree)?;
    assert_eq!(cache::eval_memo_entries_for(module_oid), 2);
    Ok(())
}

#[test]
fn test_clear_caches() -> anyhow::Result<()> {
    let _guard = cache_lock();
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))"#,
        r#"(data (i32.const 900) "clear-caches-test-unique")"#,
        "(call $nop)",
    );
    let module_oid = write_blob(&repo, wat.as_bytes())?;
    crate::evaluate(repo.odb(), module_oid, &[], tree)?;
    assert_eq!(cache::eval_memo_entries_for(module_oid), 1);
    assert!(cache::module_cache().lock().contains(&module_oid));
    crate::clear_caches();
    assert_eq!(cache::eval_memo_entries_for(module_oid), 0);
    assert!(!cache::module_cache().lock().contains(&module_oid));
    // Evaluation still works after clearing (recompiles).
    assert!(crate::evaluate(repo.odb(), module_oid, &[], tree)?.is_nop());
    Ok(())
}

#[test]
fn test_module_lru_eviction() -> anyhow::Result<()> {
    // Exercised on a local `ModuleLru` with capacity 2 instead of overriding
    // the global cache's capacity: the global caches are shared across
    // parallel tests, so shrinking them here would race.
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let engine = crate::wasmi_engine::WasmiEngine::new();
    let limits = crate::limits();
    let mut lru = cache::ModuleLru::new(2);
    let mut oids = Vec::new();
    for name in ["lru-a", "lru-b", "lru-c"] {
        let wat = module(
            r#"(import "josh" "nop" (func $nop (result i32)))"#,
            &format!(r#"(data (i32.const 900) "{name}")"#),
            "(call $nop)",
        );
        let oid = write_blob(&repo, wat.as_bytes())?;
        let compiled = engine.load(wat.as_bytes(), &limits)?;
        lru.insert(oid, compiled);
        oids.push((oid, wat));
    }
    // Capacity 2: the least recently used entry (lru-a) was evicted.
    assert_eq!(lru.len(), 2);
    assert!(!lru.contains(&oids[0].0));
    assert!(lru.contains(&oids[1].0));
    assert!(lru.contains(&oids[2].0));
    // A get refreshes recency: after touching lru-b, inserting a fourth
    // module evicts lru-c instead.
    assert!(lru.get(&oids[1].0).is_some());
    let wat_d = module(
        r#"(import "josh" "nop" (func $nop (result i32)))"#,
        r#"(data (i32.const 900) "lru-d")"#,
        "(call $nop)",
    );
    let oid_d = write_blob(&repo, wat_d.as_bytes())?;
    lru.insert(oid_d, engine.load(wat_d.as_bytes(), &limits)?);
    assert!(lru.contains(&oids[1].0));
    assert!(!lru.contains(&oids[2].0));
    // The evicted module still evaluates: it is simply recompiled.
    let recompiled = engine.load(oids[0].1.as_bytes(), &limits)?;
    let filter = recompiled.evaluate(
        EvalContext {
            odb: repo.odb(),
            tree_oid: tree,
            args: &[],
        },
        &limits,
    )?;
    assert!(filter.is_nop());
    Ok(())
}

#[test]
fn test_wasm_import() -> anyhow::Result<()> {
    // The builder is experimental-gated; enable it before the gate's LazyLock
    // is first read (no other test in this binary touches the gate).
    unsafe { std::env::set_var("JOSH_EXPERIMENTAL_FEATURES", "1") };
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))
  (import "josh" "wasm" (func $wasm (param i32 i32 i32 i32 i32 i32) (result i32)))"#,
        r#"(data (i32.const 16) "tools/mod")
  (data (i32.const 32) "a\nb")"#,
        r#"(call $wasm (call $nop) (i32.const 16) (i32.const 9) (i32.const 32) (i32.const 3) (call $nop))"#,
    );
    let filter = eval(&repo, &wat, &[], tree)?;
    assert_eq!(spec(filter), ":!tools/mod=a,b[:/]");
    Ok(())
}

#[test]
fn test_nan_bit_determinism() -> anyhow::Result<()> {
    // Engine-swap regression guard: 0.0 / 0.0 must produce the canonical
    // quiet-NaN bit pattern deterministically. An engine that does not
    // canonicalize NaNs (e.g. wasmtime without nan_canonicalization) fails
    // this test instead of silently producing nondeterministic filters.
    let _guard = cache_lock();
    let repo = temp_repo()?;
    let tree = empty_tree(&repo)?;
    let wat = module(
        r#"(import "josh" "nop" (func $nop (result i32)))
  (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))"#,
        r#"(data (i32.const 16) "ab")"#,
        r#"(if (result i32)
      (i64.eq
        (i64.reinterpret_f64 (f64.div (f64.const 0) (f64.const 0)))
        (i64.const 0x7ff8000000000000))
      (then (call $subdir (call $nop) (i32.const 16) (i32.const 1)))
      (else (call $subdir (call $nop) (i32.const 17) (i32.const 1))))"#,
    );
    let first = eval(&repo, &wat, &[], tree)?;
    assert_eq!(spec(first), ":/a");
    crate::clear_caches();
    let second = eval(&repo, &wat, &[], tree)?;
    assert_eq!(spec(second), ":/a");
    assert!(first == second);
    Ok(())
}
