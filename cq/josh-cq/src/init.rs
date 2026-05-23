use anyhow::Context;

use josh_link::make_signature;

pub fn handle_init(transaction: &josh_core::cache::Transaction) -> anyhow::Result<String> {
    let repo = transaction.repo();

    if repo.head().is_ok() {
        return Ok("Already initialized".to_string());
    }

    let head_ref = repo.find_reference("HEAD").context("Failed to find HEAD")?;
    let target = head_ref
        .symbolic_target()
        .context("HEAD is not a symbolic reference")?
        .to_string();

    let signature = make_signature(repo)?;

    let empty_tree_oid = repo
        .treebuilder(None)
        .context("Failed to create tree builder")?
        .write()
        .context("Failed to write empty tree")?;
    let empty_tree = repo
        .find_tree(empty_tree_oid)
        .context("Failed to find empty tree")?;

    let commit_oid = repo
        .commit(
            Some(&target),
            &signature,
            &signature,
            "Initialize metarepo",
            &empty_tree,
            &[],
        )
        .context("Failed to create initial commit")?;

    Ok(format!(
        "Initialized metarepo on {} at {}",
        target, commit_oid
    ))
}
