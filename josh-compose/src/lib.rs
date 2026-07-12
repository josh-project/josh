pub mod archive;
pub mod clean;
pub mod container;
pub mod filter;
pub mod image;
pub mod job_cache;
pub mod meta;
pub mod naming;
pub mod plan;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputMode {
    /// No output artifact is created; only success/failure is recorded.
    None,
    /// Output artifact is created and its contents are extracted to the host working directory.
    Workdir,
    /// Output artifact is created and kept (e.g. for use as a dependency input), but not
    /// extracted.
    Keep,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CleanMode {
    /// No cleanup.
    None,
    /// Remove output artifacts, environment images, and job-cache directories.
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

/// Main entry point for `josh run`.
pub fn run(transaction: &josh_core::cache::Transaction, opts: RunOptions) -> anyhow::Result<()> {
    josh_filter::check_experimental_features_enabled("josh run")?;

    let runtime = josh_compose_backend::PodmanRuntime::new();

    if opts.clean != CleanMode::None {
        return clean::clean(opts.clean, &runtime);
    }

    let filter_spec = opts.filter_spec.trim().to_string();
    let repo = transaction.repo();

    let source_commit = filter::resolve_input(repo, &opts.input_ref)?;

    let (ws_tree, _safe_name) = filter::compute_ws_tree(transaction, &filter_spec, source_commit)?;

    let mut attempted = std::collections::HashSet::new();
    // Only extract output artifacts into the working tree when running against
    // uncommitted changes (input_ref == "."). For committed refs there is no
    // working tree to write back to.
    let extract_to_workdir = opts.input_ref == ".";
    container::run_container(repo, ws_tree, &mut attempted, extract_to_workdir, &runtime)?;

    Ok(())
}

/// Enumerate every image build-tree OID that a `run` with the same options would
/// require, bases-first and deduplicated.
///
/// When `ignore_cache` is false, workspaces whose run is already cached successful and
/// whose output volume still exists are pruned from the walk (mirroring
/// `container::run_container`'s early-return). When `ignore_cache` is true, the full
/// set is reported regardless of cache state.
pub fn plan_images(
    transaction: &josh_core::cache::Transaction,
    opts: RunOptions,
    ignore_cache: bool,
) -> anyhow::Result<Vec<git2::Oid>> {
    josh_filter::check_experimental_features_enabled("josh compose images")?;

    let filter_spec = opts.filter_spec.trim().to_string();
    let repo = transaction.repo();

    let source_commit = filter::resolve_input(repo, &opts.input_ref)?;

    let (ws_tree, _safe_name) = filter::compute_ws_tree(transaction, &filter_spec, source_commit)?;

    let runtime = josh_compose_backend::PodmanRuntime::new();
    plan::collect_image_oids(repo, ws_tree, ignore_cache, &runtime)
}

/// Enumerate every job hash (workspace tree OID) that a `run` with the same options
/// would touch, in dependency order (dependencies first).
///
/// When `ignore_cache` is false, workspaces whose run is already cached successful and
/// whose output volume still exists are pruned from the walk (mirroring
/// `container::run_container`'s early-return). When `ignore_cache` is true, the full
/// set is reported regardless of cache state.
pub fn plan_jobs(
    transaction: &josh_core::cache::Transaction,
    opts: RunOptions,
    ignore_cache: bool,
) -> anyhow::Result<Vec<git2::Oid>> {
    josh_filter::check_experimental_features_enabled("josh compose jobs")?;

    let filter_spec = opts.filter_spec.trim().to_string();
    let repo = transaction.repo();

    let source_commit = filter::resolve_input(repo, &opts.input_ref)?;

    let (ws_tree, _safe_name) = filter::compute_ws_tree(transaction, &filter_spec, source_commit)?;

    let runtime = josh_compose_backend::PodmanRuntime::new();
    plan::collect_job_hashes(repo, ws_tree, ignore_cache, &runtime)
}
