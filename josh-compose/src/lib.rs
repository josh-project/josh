use josh_compose_backend::{ExecOpts, Executor, Runtime};

pub mod archive;
pub mod clean;
pub mod executor;
pub mod filter;
pub mod image;
pub mod job_cache;
pub mod naming;
pub mod plan;

#[derive(Debug, Clone, PartialEq)]
pub enum CleanMode {
    /// No cleanup.
    None,
    /// Remove output artifacts, environment images, and compose result metadata.
    Clean,
    /// Like `Clean`, but also remove persistent cache artifacts.
    CleanAll,
}

pub struct RunOptions {
    /// Filter spec, e.g. ":+ws/test"
    pub filter_spec: String,
    /// Input ref: "." (working tree), "+" (index), "HEAD", or any git ref
    pub input_ref: String,
    pub clean: CleanMode,
}

/// Main entry point for `josh run`, using the default sequential executor.
pub fn run(
    transaction: &josh_core::cache::Transaction,
    opts: RunOptions,
    runtime: &dyn Runtime,
) -> anyhow::Result<()> {
    run_with_executor(transaction, opts, runtime, &executor::SequentialExecutor)
}

/// Load the build graph for the given options and hand it to `executor`.
///
/// Graph loading (resolving the workspace and image dependency closure from git
/// trees) happens here, once, before the executor makes any scheduling decision.
pub fn run_with_executor(
    transaction: &josh_core::cache::Transaction,
    opts: RunOptions,
    runtime: &dyn Runtime,
    executor: &dyn Executor,
) -> anyhow::Result<()> {
    josh_filter::check_experimental_features_enabled("josh run")?;

    if opts.clean != CleanMode::None {
        return clean::clean(transaction, opts.clean, runtime);
    }

    let filter_spec = opts.filter_spec.trim().to_string();
    let source_commit = filter::resolve_input(transaction, &opts.input_ref)?;

    let (ws_tree, _safe_name) = filter::compute_ws_tree(transaction, &filter_spec, source_commit)?;

    clean::reclaim_if_needed(transaction, ws_tree, runtime)?;

    let graph = josh_compose_graph::load_graph(transaction, transaction.odb(), ws_tree)?;

    // Only extract output artifacts into the working tree when running against
    // uncommitted changes (input_ref == "."). For committed refs there is no
    // working tree to write back to.
    let exec_opts = ExecOpts {
        extract_to_workdir: opts.input_ref == ".",
    };
    executor.execute(transaction, &graph, runtime, &exec_opts)
}

/// Load the complete workspace and image dependency graph for a compose run.
pub fn load_plan(
    transaction: &josh_core::cache::Transaction,
    filter_spec: &str,
    input_ref: &str,
) -> anyhow::Result<josh_compose_graph::Graph> {
    josh_filter::check_experimental_features_enabled("josh compose graph")?;

    let filter_spec = filter_spec.trim();
    let source_commit = filter::resolve_input(transaction, input_ref)?;
    let (ws_tree, _safe_name) = filter::compute_ws_tree(transaction, filter_spec, source_commit)?;

    josh_compose_graph::load_graph(transaction, transaction.odb(), ws_tree)
}
/// Pull compose result metadata from `remote`, merging concurrent local results.
pub fn pull(transaction: &josh_core::cache::Transaction, remote: &str) -> anyhow::Result<()> {
    josh_filter::check_experimental_features_enabled("josh compose pull")?;
    job_cache::pull_results(transaction, remote)
}

/// Push compose result metadata to `remote`, merging and retrying concurrent updates.
pub fn push(transaction: &josh_core::cache::Transaction, remote: &str) -> anyhow::Result<()> {
    josh_filter::check_experimental_features_enabled("josh compose push")?;
    job_cache::push_results(transaction, remote)
}

/// Enumerate every image build-tree OID that a `run` with the same options would
/// require, bases-first and deduplicated.
///
/// When `ignore_cache` is false, workspaces whose run is already cached successful and
/// whose output volume still exists are pruned from the graph (mirroring the
/// executor's cache check). When `ignore_cache` is true, the full set is reported
/// regardless of cache state.
pub fn plan_images(
    transaction: &josh_core::cache::Transaction,
    opts: RunOptions,
    ignore_cache: bool,
    runtime: &dyn Runtime,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    josh_filter::check_experimental_features_enabled("josh compose images")?;

    let filter_spec = opts.filter_spec.trim().to_string();
    let source_commit = filter::resolve_input(transaction, &opts.input_ref)?;

    let (ws_tree, _safe_name) = filter::compute_ws_tree(transaction, &filter_spec, source_commit)?;

    let odb = transaction.odb();
    plan::collect_image_oids(transaction, odb, ws_tree, ignore_cache, runtime)
}

/// Enumerate every job hash (workspace tree OID) that a `run` with the same options
/// would touch, in dependency order (dependencies first).
///
/// When `ignore_cache` is false, workspaces whose run is already cached successful and
/// whose output volume still exists are pruned from the graph (mirroring the
/// executor's cache check). When `ignore_cache` is true, the full set is reported
/// regardless of cache state.
pub fn plan_jobs(
    transaction: &josh_core::cache::Transaction,
    opts: RunOptions,
    ignore_cache: bool,
    runtime: &dyn Runtime,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    josh_filter::check_experimental_features_enabled("josh compose jobs")?;

    let filter_spec = opts.filter_spec.trim().to_string();
    let source_commit = filter::resolve_input(transaction, &opts.input_ref)?;

    let (ws_tree, _safe_name) = filter::compute_ws_tree(transaction, &filter_spec, source_commit)?;

    let odb = transaction.odb();
    plan::collect_job_hashes(transaction, odb, ws_tree, ignore_cache, runtime)
}
