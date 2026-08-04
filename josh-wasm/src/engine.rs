//! Internal engine abstraction. The concrete runtime is wasmi; keeping the
//! surface behind these two traits is cheap insurance for a later engine swap
//! (which must preserve determinism — see the NaN regression test).

use std::sync::Arc;

/// Per-evaluation resource limits.
pub struct Limits {
    pub fuel: u64,
    pub memory_bytes: usize,
    pub module_size: usize,
    pub max_handles: usize,
}

/// Everything an evaluation may observe: the repository (for tree access), the
/// context-filtered tree and the invocation arguments.
pub struct EvalContext<'a> {
    pub repo: &'a git2::Repository,
    pub tree_oid: git2::Oid,
    pub args: &'a [String],
}

pub trait Engine: Send + Sync {
    /// Validate and compile a wasm binary.
    fn load(&self, bytes: &[u8], limits: &Limits) -> anyhow::Result<Arc<dyn CompiledModule>>;
}

pub trait CompiledModule: Send + Sync {
    /// Instantiate the module and run it to completion, returning the filter
    /// handle produced by the guest's `josh_run`.
    fn evaluate(
        &self,
        ctx: EvalContext<'_>,
        limits: &Limits,
    ) -> anyhow::Result<josh_filter::Filter>;
}
