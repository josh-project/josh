use anyhow::Context;

use josh_core::git::spawn_git_command;
use josh_link::make_signature;

use crate::types::UserAction;

pub fn default_mode() -> String {
    "snapshot".to_string()
}

pub fn handle_track(
    url: &str,
    id: &str,
    mode: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<UserAction> {
    let repo = transaction.repo();

    spawn_git_command(repo.path(), &["fetch", url, "HEAD"], &[])?;

    let fetch_head_ref = repo
        .find_reference("FETCH_HEAD")
        .context("Failed to find FETCH_HEAD")?;
    let fetched_commit = fetch_head_ref
        .peel_to_commit()
        .context("Failed to peel FETCH_HEAD to commit")?
        .id();

    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let signature = make_signature(repo)?;

    let link_mode = josh_core::filter::LinkMode::parse(mode)
        .with_context(|| format!("Invalid link mode: '{}'", mode))?;

    let link_path = std::path::Path::new("remotes").join(id).join("link");
    let tree_oid = josh_link::prepare_link_add(
        transaction,
        &link_path,
        url,
        None,
        "HEAD",
        fetched_commit,
        &head_tree,
        link_mode,
    )?
    .into_tree_oid();

    let tree = repo
        .find_tree(tree_oid)
        .context("Failed to find tree with link")?;

    let commit = repo
        .commit(
            None,
            &signature,
            &signature,
            &format!("Track remote: {}", id),
            &tree,
            &[&head_commit],
        )
        .context("Failed to create final commit")?;

    repo.head()?
        .set_target(commit, "josh-cq track")
        .context("Failed to update HEAD")?;

    let action = UserAction::Message(format!("Tracked remote '{}' at {}", id, url));

    Ok(action)
}
