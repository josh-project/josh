pub mod archive;
pub mod clean;
pub mod container;
pub mod filter;
pub mod image;
pub mod job_cache;
pub mod meta;
pub mod podman;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputMode {
    /// No output volume is created; only success/failure is recorded.
    None,
    /// Output volume is created and its contents are copied back to the host working directory.
    Workdir,
    /// Output volume is created and kept (e.g. for use as a dependency input), but not extracted.
    Keep,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CleanMode {
    None,
    Clean,
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

    if opts.clean != CleanMode::None {
        return clean::clean(opts.clean);
    }

    let filter_spec = opts.filter_spec.trim().to_string();
    let repo = transaction.repo();
    // Keep temporary filter objects in memory; never write loose objects to disk.
    let _mempack = repo.odb()?.add_new_mempack_backend(1000)?;

    let source_commit = filter::resolve_input(repo, &opts.input_ref)?;
    let version = filter::git_version(repo);

    let (ws_tree, _safe_name) =
        filter::compute_ws_tree(transaction, &filter_spec, source_commit, &version)?;

    container::run_container(repo, ws_tree)?;

    Ok(())
}
