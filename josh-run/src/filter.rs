use anyhow::Context;

/// Resolve an input ref to a commit OID.
/// Delegates to josh_core::git::resolve_snapshot_input.
pub fn resolve_input(repo: &git2::Repository, input_ref: &str) -> anyhow::Result<git2::Oid> {
    josh_core::git::resolve_snapshot_input(repo, input_ref)
        .with_context(|| format!("failed to resolve input ref: {input_ref:?}"))
}

/// Get a version string for the repo using `git describe --tags --always`.
pub fn git_version(repo: &git2::Repository) -> String {
    let mut opts = git2::DescribeOptions::new();
    opts.describe_tags();
    repo.describe(&opts)
        .and_then(|d| d.format(None))
        .unwrap_or_else(|_| {
            repo.head()
                .ok()
                .and_then(|h| h.target())
                .map(|oid| oid.to_string()[..12].to_string())
                .unwrap_or_else(|| "unknown".to_string())
        })
}

/// Compute the workspace tree OID and safe name for the given filter spec.
///
/// Constructs the filter:
///   :SQUASH:#X[:/,:\$VERSION_STRING="<ver>"]:#/X<user_filter>
///
/// Returns (ws_tree_oid, safe_name).
pub fn compute_ws_tree(
    transaction: &josh_core::cache::Transaction,
    filter_spec: &str,
    source_commit: git2::Oid,
    version_string: &str,
) -> anyhow::Result<(git2::Oid, String)> {
    let repo = transaction.repo();

    // Escape the version string for embedding in a filter spec
    let version_escaped = version_string.replace('\\', "\\\\").replace('"', "\\\"");

    // Build the filter string: :SQUASH:#X[:/,:\$VERSION_STRING="<ver>"]:#/X<filter>
    // Use push('$') to avoid Rust string escaping ambiguity with the :$ blob operator.
    let full_filter = {
        let mut s = ":SQUASH:#X[:/,:".to_string();
        s.push('$');
        s.push_str(&format!(
            "VERSION_STRING=\"{version_escaped}\"]:#/X{filter_spec}"
        ));
        s
    };

    let filterobj = josh_core::filter::parse(&full_filter)
        .with_context(|| format!("failed to parse filter: {full_filter:?}"))?;

    let filtered_commit = josh_core::filter_commit(transaction, filterobj, source_commit)
        .context("failed to apply filter")?;

    let ws_tree = repo
        .find_commit(filtered_commit)
        .context("filtered result is not a commit")?
        .tree_id();

    let safe_name = josh_core::filter::as_tree(repo, filterobj)
        .context("failed to compute filter id")?
        .to_string();

    Ok((ws_tree, safe_name))
}
