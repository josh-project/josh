//! WASM filter evaluation runtime.
//!
//! A filter module is a wasm blob (or WAT text, assembled on the fly) stored in
//! the commit tree being filtered. [`evaluate`] instantiates it in a sandboxed,
//! deterministic runtime (no WASI, fuel- and memory-limited), gives it read-only
//! access to the context-filtered tree and returns the `josh_filter::Filter` the
//! module constructed through the `"josh"` host imports.
//!
//! All failures bubble up as errors; degrading evaluation errors to `Op::Empty`
//! is the caller's (josh-core's) job.

mod cache;
mod engine;
mod host;
mod wasmi_engine;

#[cfg(test)]
mod tests;

use engine::{Engine, EvalContext, Limits};
use std::sync::{Arc, LazyLock};

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Fuel budget (roughly: instructions) per evaluation.
pub static JOSH_WASM_FUEL: LazyLock<u64> = LazyLock::new(|| env_or("JOSH_WASM_FUEL", 100_000_000));

/// Maximum linear memory per instance, in bytes.
pub static JOSH_WASM_MEMORY_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_or("JOSH_WASM_MEMORY_LIMIT", 64 << 20));

/// Maximum size of a module blob, in bytes.
pub static JOSH_WASM_MODULE_SIZE_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_or("JOSH_WASM_MODULE_SIZE_LIMIT", 16 << 20));

/// Capacity (entries) of the in-memory compiled-module LRU cache.
pub static JOSH_WASM_MODULE_CACHE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_or("JOSH_WASM_MODULE_CACHE_SIZE", 64));

/// Maximum nesting depth of wasm filter evaluations (enforced by josh-core).
pub static JOSH_WASM_MAX_DEPTH: LazyLock<usize> =
    LazyLock::new(|| env_or("JOSH_WASM_MAX_DEPTH", 10));

/// Maximum number of filter handles per evaluation. Every constructed filter
/// is interned process-wide for the process lifetime, so this bounds the
/// permanent host memory a single evaluation can allocate.
pub static JOSH_WASM_MAX_HANDLES: LazyLock<usize> =
    LazyLock::new(|| env_or("JOSH_WASM_MAX_HANDLES", 100_000));

/// A stable string identifying the resource-limit configuration. Evaluation
/// outcomes depend on these limits (exhaustion degrades to an error), so any
/// cache of results that outlives the process configuration must include this
/// in its keys.
pub fn limits_fingerprint() -> &'static str {
    static FINGERPRINT: LazyLock<String> = LazyLock::new(|| {
        format!(
            "fuel={},memory={},module_size={},handles={},depth={}",
            *JOSH_WASM_FUEL,
            *JOSH_WASM_MEMORY_LIMIT,
            *JOSH_WASM_MODULE_SIZE_LIMIT,
            *JOSH_WASM_MAX_HANDLES,
            *JOSH_WASM_MAX_DEPTH
        )
    });
    &FINGERPRINT
}

static WASMI: LazyLock<wasmi_engine::WasmiEngine> = LazyLock::new(wasmi_engine::WasmiEngine::new);

fn limits() -> Limits {
    Limits {
        fuel: *JOSH_WASM_FUEL,
        memory_bytes: *JOSH_WASM_MEMORY_LIMIT,
        module_size: *JOSH_WASM_MODULE_SIZE_LIMIT,
        max_handles: *JOSH_WASM_MAX_HANDLES,
    }
}

fn load_module(
    repo: &git2::Repository,
    module_oid: git2::Oid,
    limits: &Limits,
) -> anyhow::Result<Arc<dyn engine::CompiledModule>> {
    if let Some(module) = cache::module_cache().lock().unwrap().get(&module_oid) {
        return Ok(module);
    }
    let blob = repo.find_blob(module_oid)?;
    let content = blob.content();
    if content.len() > limits.module_size {
        anyhow::bail!(
            "wasm module {} exceeds size limit: {} > {}",
            module_oid,
            content.len(),
            limits.module_size
        );
    }
    // A blob without the wasm magic that is valid UTF-8 is treated as WAT text.
    let bytes = if content.starts_with(b"\0asm") {
        std::borrow::Cow::Borrowed(content)
    } else {
        let text = std::str::from_utf8(content).map_err(|_| {
            anyhow::anyhow!(
                "wasm module {} is neither a wasm binary nor UTF-8 WAT text",
                module_oid
            )
        })?;
        std::borrow::Cow::Owned(wat::parse_str(text)?)
    };
    let module = WASMI.load(&bytes, limits)?;
    tracing::trace!("compiled wasm module {}", module_oid);
    cache::module_cache()
        .lock()
        .unwrap()
        .insert(module_oid, module.clone());
    Ok(module)
}

/// Evaluate the wasm filter module stored in blob `module_oid`, giving it
/// read-only access to the (context-filtered) tree `tree_oid` and the
/// invocation `args`, and return the filter it constructs.
pub fn evaluate(
    repo: &git2::Repository,
    module_oid: git2::Oid,
    args: &[String],
    tree_oid: git2::Oid,
) -> anyhow::Result<josh_filter::Filter> {
    let key = (module_oid, args.to_vec(), tree_oid);
    if let Some(filter) = cache::eval_memo().lock().unwrap().get(&key) {
        return Ok(*filter);
    }
    let limits = limits();
    let module = load_module(repo, module_oid, &limits)?;
    let filter = module.evaluate(
        EvalContext {
            repo,
            tree_oid,
            args,
        },
        &limits,
    )?;
    cache::eval_memo().lock().unwrap().insert(key, filter);
    Ok(filter)
}

/// Clear the compiled-module cache and the evaluation memo.
pub fn clear_caches() {
    cache::clear();
}
