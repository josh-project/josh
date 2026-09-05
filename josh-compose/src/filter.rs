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
/// Returns (ws_tree_oid, safe_name).
pub fn compute_ws_tree(
    transaction: &josh_core::cache::Transaction,
    filter_spec: &str,
    source_commit: gix_hash::ObjectId,
) -> anyhow::Result<(gix_hash::ObjectId, String)> {
    let filterobj = josh_core::filter::parse(filter_spec)
        .with_context(|| format!("failed to parse filter: {filter_spec:?}"))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn open_transaction(td: &tempfile::TempDir) -> anyhow::Result<josh_core::cache::Transaction> {
        let cache = josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::new(td.path()));
        Ok(josh_core::cache::TransactionContext::new(td.path(), cache.into()).open()?)
    }

    #[test]
    fn applies_the_workspace_filter_without_squashing() -> anyhow::Result<()> {
        let td = tempfile::tempdir()?;
        gix::init_bare(td.path())?;
        let transaction = open_transaction(&td)?;
        let signature = josh_core::git::josh_actor_signature()?;
        let source_commit = josh_core::objects::write_commit(
            transaction.odb(),
            gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1),
            &[],
            &signature,
            &signature,
            "source",
        )?;

        let (_, safe_name) = compute_ws_tree(&transaction, ":/workspace", source_commit)?;
        let expected_filter = josh_core::filter::parse(":/workspace")?;
        let expected_name = josh_core::filter::as_tree(&transaction, expected_filter)?.to_string();

        assert_eq!(safe_name, expected_name);
        Ok(())
    }
}
