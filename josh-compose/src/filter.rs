use anyhow::Context;

/// Resolve an input ref to a commit OID.
/// Delegates to josh_core::git::resolve_snapshot_input.
pub fn resolve_input(
    transaction: &josh_core::cache::Transaction,
    input_ref: &str,
) -> anyhow::Result<gix_hash::ObjectId> {
    josh_core::git::resolve_snapshot_input(transaction, input_ref)
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
    source_commit: gix_hash::ObjectId,
) -> anyhow::Result<(gix_hash::ObjectId, String)> {
    let full_filter = format!(":SQUASH{filter_spec}");

    let filterobj = josh_core::filter::parse(&full_filter)
        .with_context(|| format!("failed to parse filter: {full_filter:?}"))?;

    let filtered_commit = josh_core::filter_commit(transaction, filterobj, source_commit)
        .context("failed to apply filter")?;

    let odb = transaction.odb();
    let ws_tree = josh_core::objects::CommitData::read(odb, filtered_commit)
        .context("filtered result is not a commit")?
        .tree_id()?;

    let safe_name = josh_core::filter::as_tree(transaction, filterobj)
        .context("failed to compute filter id")?
        .to_string();

    Ok((ws_tree, safe_name))
}
