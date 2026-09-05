//! Host state and the `"josh"` import module.
//!
//! Builder imports delegate 1:1 to `josh_filter::Filter` methods; the guest
//! only ever holds opaque `u32` handles into the per-evaluation handle table.
//! Tree imports operate on the context-filtered tree with no way outside it.
//! Any invalid handle, non-UTF-8 string or out-of-bounds pointer traps, which
//! surfaces as an evaluation error.

use crate::engine::{EvalContext, Limits};
use josh_filter::Filter;
use std::path::Path;
use wasmi::{Caller, Extern, Linker, Memory};

pub(crate) struct HostState {
    // Evaluation is fully synchronous: `CompiledModule::evaluate` creates the
    // store, runs the guest to completion and drops the store before
    // returning, so the repository always outlives this pointer. A raw
    // pointer (instead of `&'a git2::Repository`) is needed because wasmi's
    // host function closures must be `'static`, which forbids a lifetime
    // parameter on the store data type.
    repo: *const git2::Repository,
    tree_oid: git2::Oid,
    args: Vec<String>,
    handles: Vec<Filter>,
    max_handles: usize,
    memory_bytes: usize,
    store_limits: wasmi::StoreLimits,
}

impl HostState {
    pub(crate) fn new(ctx: &EvalContext<'_>, limits: &Limits) -> HostState {
        HostState {
            repo: ctx.repo as *const _,
            tree_oid: ctx.tree_oid,
            args: ctx.args.to_vec(),
            handles: Vec::new(),
            max_handles: limits.max_handles,
            memory_bytes: limits.memory_bytes,
            // The memory cap alone does not bound host allocations: wasmi
            // eagerly commits a table's declared minimum at instantiation
            // (before any fuel is consumed), and tables/memories are separate
            // budgets counted per instance. Cap everything a module can make
            // the store allocate: one instance, one memory, one table, and
            // table elements (8 bytes each) budgeted like linear memory.
            store_limits: wasmi::StoreLimitsBuilder::new()
                .memory_size(limits.memory_bytes)
                .table_elements(limits.memory_bytes / 8)
                .instances(1)
                .memories(1)
                .tables(1)
                .trap_on_grow_failure(true)
                .build(),
        }
    }

    pub(crate) fn limiter(&mut self) -> &mut dyn wasmi::ResourceLimiter {
        &mut self.store_limits
    }

    pub(crate) fn handle(&self, handle: u32) -> Option<Filter> {
        self.handles.get(handle as usize).copied()
    }
}

type HCaller<'c> = Caller<'c, HostState>;

fn repo<'c>(caller: &'c HCaller<'_>) -> &'c git2::Repository {
    // SAFETY: see the `HostState::repo` field documentation.
    unsafe { &*caller.data().repo }
}

fn memory(caller: &HCaller<'_>) -> Result<Memory, wasmi::Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("guest does not export \"memory\""))
}

fn read_bytes<'c>(caller: &'c HCaller<'_>, ptr: u32, len: usize) -> Result<&'c [u8], wasmi::Error> {
    let data = memory(caller)?.data(caller);
    let start = ptr as usize;
    let end = start
        .checked_add(len)
        .ok_or_else(|| wasmi::Error::new("guest pointer range overflows"))?;
    data.get(start..end)
        .ok_or_else(|| wasmi::Error::new("guest pointer out of bounds"))
}

fn read_string(caller: &HCaller<'_>, ptr: u32, len: u32) -> Result<String, wasmi::Error> {
    let bytes = read_bytes(caller, ptr, len as usize)?;
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|_| wasmi::Error::new("guest string is not valid UTF-8"))
}

fn get_handle(caller: &HCaller<'_>, handle: u32) -> Result<Filter, wasmi::Error> {
    caller
        .data()
        .handle(handle)
        .ok_or_else(|| wasmi::Error::new(format!("invalid filter handle: {}", handle)))
}

/// Register a filter in the handle table. Capped (`JOSH_WASM_MAX_HANDLES`):
/// every constructed filter is interned process-wide for the process lifetime,
/// so unbounded construction would leak host memory that neither the fuel nor
/// the guest memory limit accounts for.
fn push_handle(caller: &mut HCaller<'_>, filter: Filter) -> Result<u32, wasmi::Error> {
    let state = caller.data_mut();
    if state.handles.len() >= state.max_handles {
        return Err(wasmi::Error::new("filter handle limit exceeded"));
    }
    state.handles.push(filter);
    Ok((state.handles.len() - 1) as u32)
}

/// Write a host string into a guest buffer obtained by re-entrantly calling
/// the guest's exported `josh_alloc`, returning `(ptr << 32) | len`. The
/// empty string is returned as `0` without touching the guest.
fn return_string(caller: &mut HCaller<'_>, s: &str) -> Result<i64, wasmi::Error> {
    if s.is_empty() {
        return Ok(0);
    }
    let alloc = caller
        .get_export("josh_alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| wasmi::Error::new("guest does not export \"josh_alloc\""))?
        .typed::<u32, u32>(&*caller)?;
    let ptr = alloc.call(&mut *caller, s.len() as u32)?;
    memory(caller)?.write(&mut *caller, ptr as usize, s.as_bytes())?;
    Ok((((ptr as u64) << 32) | s.len() as u64) as i64)
}

fn host_err(e: impl std::fmt::Display) -> wasmi::Error {
    wasmi::Error::new(e.to_string())
}

/// Blob content at `path`; empty string for absent, non-blob, binary,
/// non-UTF-8 or oversized (larger than `max_bytes`) entries.
pub(crate) fn tree_file_content(
    repo: &git2::Repository,
    tree_oid: git2::Oid,
    path: &str,
    max_bytes: usize,
) -> String {
    let Ok(tree) = repo.find_tree(tree_oid) else {
        return String::new();
    };
    let Ok(entry) = tree.get_path(Path::new(path)) else {
        return String::new();
    };
    if entry.kind() != Some(git2::ObjectType::Blob) {
        return String::new();
    }
    // Check the size from the odb header BEFORE loading the blob: a blob that
    // could never be delivered into guest memory must not be decompressed and
    // copied host-side either — none of fuel, module size or the guest memory
    // limit would otherwise bound this allocation.
    let Ok(odb) = repo.odb() else {
        return String::new();
    };
    match odb.read_header(entry.id()) {
        Ok((size, _)) if size <= max_bytes => {}
        _ => return String::new(),
    }
    let Ok(blob) = repo.find_blob(entry.id()) else {
        return String::new();
    };
    if blob.is_binary() {
        return String::new();
    }
    std::str::from_utf8(blob.content())
        .map(str::to_string)
        .unwrap_or_default()
}

fn tree_at<'r>(
    repo: &'r git2::Repository,
    tree_oid: git2::Oid,
    path: &str,
) -> Option<git2::Tree<'r>> {
    let root = repo.find_tree(tree_oid).ok()?;
    if path.is_empty() {
        return Some(root);
    }
    let entry = root.get_path(Path::new(path)).ok()?;
    if entry.kind() != Some(git2::ObjectType::Tree) {
        return None;
    }
    repo.find_tree(entry.id()).ok()
}

/// Newline-joined full slash-joined paths of the immediate child entries of
/// `kind` at `path`, in git tree entry order. Non-UTF-8 names and paths
/// containing a newline are skipped; a bad path yields an empty result.
fn tree_entry_list(
    repo: &git2::Repository,
    tree_oid: git2::Oid,
    path: &str,
    kind: git2::ObjectType,
) -> String {
    let Some(tree) = tree_at(repo, tree_oid, path) else {
        return String::new();
    };
    let mut entries = Vec::new();
    for entry in tree.iter() {
        if entry.kind() != Some(kind) {
            continue;
        }
        let Ok(name) = entry.name() else {
            continue;
        };
        let full = if path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", path, name)
        };
        if full.contains('\n') {
            continue;
        }
        entries.push(full);
    }
    entries.join("\n")
}

/// Hex OID of the entry at `path`; the tree's own OID for the empty path;
/// empty string if absent.
fn tree_entry_oid_hex(repo: &git2::Repository, tree_oid: git2::Oid, path: &str) -> String {
    if path.is_empty() {
        return tree_oid.to_string();
    }
    let Ok(tree) = repo.find_tree(tree_oid) else {
        return String::new();
    };
    match tree.get_path(Path::new(path)) {
        Ok(entry) => entry.id().to_string(),
        Err(_) => String::new(),
    }
}

pub(crate) fn add_to_linker(linker: &mut Linker<HostState>) -> anyhow::Result<()> {
    // Constructors (no receiver).
    linker.func_wrap(
        "josh",
        "nop",
        |mut caller: HCaller| -> Result<u32, wasmi::Error> {
            push_handle(&mut caller, Filter::new())
        },
    )?;
    linker.func_wrap(
        "josh",
        "empty",
        |mut caller: HCaller| -> Result<u32, wasmi::Error> {
            push_handle(&mut caller, Filter::new().empty())
        },
    )?;

    // Combinators.
    linker.func_wrap(
        "josh",
        "chain",
        |mut caller: HCaller, a: u32, b: u32| -> Result<u32, wasmi::Error> {
            let a = get_handle(&caller, a)?;
            let b = get_handle(&caller, b)?;
            push_handle(&mut caller, a.chain(b))
        },
    )?;
    linker.func_wrap(
        "josh",
        "compose",
        |mut caller: HCaller, ptr: u32, count: u32| -> Result<u32, wasmi::Error> {
            let len = (count as usize)
                .checked_mul(4)
                .ok_or_else(|| wasmi::Error::new("compose handle count overflows"))?;
            let handles: Vec<u32> = read_bytes(&caller, ptr, len)?
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .collect();
            let mut filters = Vec::with_capacity(handles.len());
            for handle in handles {
                filters.push(get_handle(&caller, handle)?);
            }
            push_handle(&mut caller, josh_filter::compose(&filters))
        },
    )?;

    // Builder methods (receiver handle first).
    linker.func_wrap(
        "josh",
        "subdir",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, ptr, len)?;
            push_handle(&mut caller, f.subdir(path))
        },
    )?;
    linker.func_wrap(
        "josh",
        "prefix",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, ptr, len)?;
            push_handle(&mut caller, f.prefix(path))
        },
    )?;
    linker.func_wrap(
        "josh",
        "file",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, ptr, len)?;
            push_handle(&mut caller, f.file(path))
        },
    )?;
    // NOTE: destination first, matching `Filter::rename`.
    linker.func_wrap(
        "josh",
        "rename",
        |mut caller: HCaller,
         f: u32,
         dst_ptr: u32,
         dst_len: u32,
         src_ptr: u32,
         src_len: u32|
         -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let dst = read_string(&caller, dst_ptr, dst_len)?;
            let src = read_string(&caller, src_ptr, src_len)?;
            push_handle(&mut caller, f.rename(dst, src))
        },
    )?;
    linker.func_wrap(
        "josh",
        "pattern",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let glob = read_string(&caller, ptr, len)?;
            let f = f.pattern(&glob).map_err(host_err)?;
            push_handle(&mut caller, f)
        },
    )?;
    linker.func_wrap(
        "josh",
        "linear",
        |mut caller: HCaller, f: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            push_handle(&mut caller, f.linear())
        },
    )?;
    linker.func_wrap(
        "josh",
        "workspace",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, ptr, len)?;
            push_handle(&mut caller, f.workspace(path))
        },
    )?;
    linker.func_wrap(
        "josh",
        "stored",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, ptr, len)?;
            push_handle(&mut caller, f.stored(path))
        },
    )?;
    linker.func_wrap(
        "josh",
        "author",
        |mut caller: HCaller,
         f: u32,
         name_ptr: u32,
         name_len: u32,
         email_ptr: u32,
         email_len: u32|
         -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let name = read_string(&caller, name_ptr, name_len)?;
            let email = read_string(&caller, email_ptr, email_len)?;
            push_handle(&mut caller, f.author(name, email))
        },
    )?;
    linker.func_wrap(
        "josh",
        "committer",
        |mut caller: HCaller,
         f: u32,
         name_ptr: u32,
         name_len: u32,
         email_ptr: u32,
         email_len: u32|
         -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let name = read_string(&caller, name_ptr, name_len)?;
            let email = read_string(&caller, email_ptr, email_len)?;
            push_handle(&mut caller, f.committer(name, email))
        },
    )?;
    linker.func_wrap(
        "josh",
        "message",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let message = read_string(&caller, ptr, len)?;
            push_handle(&mut caller, f.message(&message))
        },
    )?;
    linker.func_wrap(
        "josh",
        "unsign",
        |mut caller: HCaller, f: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            push_handle(&mut caller, f.unsign())
        },
    )?;
    linker.func_wrap(
        "josh",
        "prune_trivial_merge",
        |mut caller: HCaller, f: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            push_handle(&mut caller, f.prune_trivial_merge())
        },
    )?;
    linker.func_wrap(
        "josh",
        "hook",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let hook = read_string(&caller, ptr, len)?;
            push_handle(&mut caller, f.hook(&hook))
        },
    )?;
    linker.func_wrap(
        "josh",
        "with_meta",
        |mut caller: HCaller,
         f: u32,
         key_ptr: u32,
         key_len: u32,
         value_ptr: u32,
         value_len: u32|
         -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let key = read_string(&caller, key_ptr, key_len)?;
            let value = read_string(&caller, value_ptr, value_len)?;
            push_handle(&mut caller, f.with_meta(key, value))
        },
    )?;
    linker.func_wrap(
        "josh",
        "insert",
        |mut caller: HCaller,
         f: u32,
         path_ptr: u32,
         path_len: u32,
         content_ptr: u32,
         content_len: u32|
         -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, path_ptr, path_len)?;
            let content = read_string(&caller, content_ptr, content_len)?;
            let f = f.insert(path, content).map_err(host_err)?;
            push_handle(&mut caller, f)
        },
    )?;
    linker.func_wrap(
        "josh",
        "treeid",
        |mut caller: HCaller, f: u32, ptr: u32, len: u32, sub: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, ptr, len)?;
            let sub = get_handle(&caller, sub)?;
            let f = f.treeid(path, sub).map_err(host_err)?;
            push_handle(&mut caller, f)
        },
    )?;
    // Args are passed newline-joined; an empty string means no args.
    linker.func_wrap(
        "josh",
        "wasm",
        |mut caller: HCaller,
         f: u32,
         path_ptr: u32,
         path_len: u32,
         args_ptr: u32,
         args_len: u32,
         ctx: u32|
         -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            let path = read_string(&caller, path_ptr, path_len)?;
            let args_str = read_string(&caller, args_ptr, args_len)?;
            let args: Vec<String> = if args_str.is_empty() {
                Vec::new()
            } else {
                args_str.split('\n').map(str::to_string).collect()
            };
            let ctx = get_handle(&caller, ctx)?;
            let f = f.wasm(path, args, ctx).map_err(host_err)?;
            push_handle(&mut caller, f)
        },
    )?;
    linker.func_wrap(
        "josh",
        "peel",
        |mut caller: HCaller, f: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            push_handle(&mut caller, f.peel())
        },
    )?;
    linker.func_wrap(
        "josh",
        "is_nop",
        |caller: HCaller, f: u32| -> Result<u32, wasmi::Error> {
            let f = get_handle(&caller, f)?;
            Ok(f.is_nop() as u32)
        },
    )?;

    // Tree access (context-filtered tree only).
    linker.func_wrap(
        "josh",
        "tree_file",
        |mut caller: HCaller, ptr: u32, len: u32| -> Result<i64, wasmi::Error> {
            let path = read_string(&caller, ptr, len)?;
            let content = tree_file_content(
                repo(&caller),
                caller.data().tree_oid,
                &path,
                caller.data().memory_bytes,
            );
            return_string(&mut caller, &content)
        },
    )?;
    linker.func_wrap(
        "josh",
        "tree_files",
        |mut caller: HCaller, ptr: u32, len: u32| -> Result<i64, wasmi::Error> {
            let path = read_string(&caller, ptr, len)?;
            let list = tree_entry_list(
                repo(&caller),
                caller.data().tree_oid,
                &path,
                git2::ObjectType::Blob,
            );
            return_string(&mut caller, &list)
        },
    )?;
    linker.func_wrap(
        "josh",
        "tree_dirs",
        |mut caller: HCaller, ptr: u32, len: u32| -> Result<i64, wasmi::Error> {
            let path = read_string(&caller, ptr, len)?;
            let list = tree_entry_list(
                repo(&caller),
                caller.data().tree_oid,
                &path,
                git2::ObjectType::Tree,
            );
            return_string(&mut caller, &list)
        },
    )?;
    linker.func_wrap(
        "josh",
        "tree_entry_oid",
        |mut caller: HCaller, ptr: u32, len: u32| -> Result<i64, wasmi::Error> {
            let path = read_string(&caller, ptr, len)?;
            let hex = tree_entry_oid_hex(repo(&caller), caller.data().tree_oid, &path);
            return_string(&mut caller, &hex)
        },
    )?;

    // Invocation context.
    linker.func_wrap(
        "josh",
        "invocation_args",
        |mut caller: HCaller| -> Result<i64, wasmi::Error> {
            let joined = caller
                .data()
                .args
                .iter()
                .filter(|arg| !arg.contains('\n'))
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            return_string(&mut caller, &joined)
        },
    )?;

    Ok(())
}
