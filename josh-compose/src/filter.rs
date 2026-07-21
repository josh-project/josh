use anyhow::Context;

/// Resolve an input ref to a commit OID.
/// Delegates to josh_core::git::resolve_snapshot_input.
pub fn resolve_input(repo: &git2::Repository, input_ref: &str) -> anyhow::Result<git2::Oid> {
    josh_core::git::resolve_snapshot_input(repo, input_ref)
        .with_context(|| format!("failed to resolve input ref: {input_ref:?}"))
}

/// Compute the workspace tree OID and safe name for the given filter spec.
///
/// Constructs the filter `:SQUASH<user_filter>` and applies it to `source_commit`.
///
/// Returns (ws_tree_oid, safe_name).
pub fn compute_ws_tree(
    transaction: &josh_core::cache::Transaction,
    filter_spec: &str,
    source_commit: git2::Oid,
) -> anyhow::Result<(git2::Oid, String)> {
    let repo = transaction.repo();

    let full_filter = format!(":SQUASH{filter_spec}");

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
