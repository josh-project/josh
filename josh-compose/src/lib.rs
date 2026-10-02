pub use filter::ArgumentBinding;
use josh_compose_backend::{ExecOpts, Executor, Runtime, SelectedJobOverride, StdioMode};

pub mod archive;
pub mod clean;
pub mod executor;
pub mod filter;
pub mod image;
pub mod job_cache;
pub mod naming;
pub mod plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanMode {
    /// Remove output artifacts, environment images, and compose result metadata.
    Clean,
    /// Like `Clean`, but also remove persistent cache artifacts.
    CleanAll,
}

pub struct WorkspaceOptions {
    /// Filter spec, e.g. ":+ws/test"
    pub filter_spec: String,
    /// Input ref: "." (working tree), "+" (index), "HEAD", or any git ref
    pub input_ref: String,
    /// Named arguments supplied as `--arg NAME=VALUE`.
    pub arguments: Vec<ArgumentBinding>,
}

/// Run the selected graph, reusing successful configured results.
///
/// A command override always executes and does not replace the configured result or output artifact.
pub fn run(
    transaction: &josh_core::cache::Transaction,
    opts: WorkspaceOptions,
    command: Option<Vec<String>>,
    runtime: &dyn Runtime,
) -> anyhow::Result<()> {
    let selected_job_override = command.map(|command| SelectedJobOverride {
        command,
        stdio: StdioMode::Capture,
    });
    execute(
        transaction,
        opts,
        runtime,
        &executor::SequentialExecutor,
        selected_job_override,
        "josh compose run",
    )
}

/// Open an interactive command in the selected workspace without recording a result.
pub fn shell(
    transaction: &josh_core::cache::Transaction,
    opts: WorkspaceOptions,
    command: Vec<String>,
    runtime: &dyn Runtime,
) -> anyhow::Result<()> {
    execute(
        transaction,
        opts,
        runtime,
        &executor::SequentialExecutor,
        Some(SelectedJobOverride {
            command,
            stdio: StdioMode::Interactive,
        }),
        "josh compose shell",
    )
}

/// Load the workspace graph and hand it to `executor`.
pub fn execute(
    transaction: &josh_core::cache::Transaction,
    opts: WorkspaceOptions,
    runtime: &dyn Runtime,
    executor: &dyn Executor,
    selected_job_override: Option<SelectedJobOverride>,
    feature_name: &str,
) -> anyhow::Result<()> {
    josh_filter::check_experimental_features_enabled(feature_name)?;

    let (ws_tree, _safe_name) = filter::prepare_workspace(
        transaction,
        &opts.filter_spec,
        &opts.input_ref,
        &opts.arguments,
    )?;

    clean::reclaim_if_needed(transaction, ws_tree, runtime)?;

    let graph = josh_compose_graph::load_graph(transaction, transaction.odb(), ws_tree)?;

    // Ephemeral command overrides never publish or extract the selected target's output.
    let exec_opts = ExecOpts {
        extract_to_workdir: opts.input_ref == "." && selected_job_override.is_none(),
        selected_job_override,
    };
    executor.execute(transaction, &graph, runtime, &exec_opts)
}

/// Load the complete workspace and image dependency graph for a compose command.
pub fn load_plan(
    transaction: &josh_core::cache::Transaction,
    filter_spec: &str,
    input_ref: &str,
    arguments: &[ArgumentBinding],
) -> anyhow::Result<josh_compose_graph::Graph> {
    josh_filter::check_experimental_features_enabled("josh compose graph")?;

    let (ws_tree, _safe_name) =
        filter::prepare_workspace(transaction, filter_spec, input_ref, arguments)?;

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
/// When `ignore_cache` is false, workspaces whose result is already cached successful and
/// whose output volume still exists are pruned from the graph (mirroring the
/// executor's cache check). When `ignore_cache` is true, the full set is reported
/// regardless of cache state.
pub fn plan_images(
    transaction: &josh_core::cache::Transaction,
    opts: WorkspaceOptions,
    ignore_cache: bool,
    runtime: &dyn Runtime,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    josh_filter::check_experimental_features_enabled("josh compose images")?;

    let (ws_tree, _safe_name) = filter::prepare_workspace(
        transaction,
        &opts.filter_spec,
        &opts.input_ref,
        &opts.arguments,
    )?;

    let odb = transaction.odb();
    plan::collect_image_oids(transaction, odb, ws_tree, ignore_cache, runtime)
}

/// Enumerate every job hash (workspace tree OID) that a `run` with the same options
/// would touch, in dependency order (dependencies first).
///
/// When `ignore_cache` is false, workspaces whose result is already cached successful and
/// whose output volume still exists are pruned from the graph (mirroring the
/// executor's cache check). When `ignore_cache` is true, the full set is reported
/// regardless of cache state.
pub fn plan_jobs(
    transaction: &josh_core::cache::Transaction,
    opts: WorkspaceOptions,
    ignore_cache: bool,
    runtime: &dyn Runtime,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    josh_filter::check_experimental_features_enabled("josh compose jobs")?;

    let (ws_tree, _safe_name) = filter::prepare_workspace(
        transaction,
        &opts.filter_spec,
        &opts.input_ref,
        &opts.arguments,
    )?;

    let odb = transaction.odb();
    plan::collect_job_hashes(transaction, odb, ws_tree, ignore_cache, runtime)
}
