use std::path::Path;

use anyhow::Context;
use git_tree_trace::trace_commit;

use josh_core::git::spawn_git_command;

use crate::layout::{self, RemoteMeta};
use crate::types::UserAction;
use crate::util::make_signature;

/// File mode for a regular (non-executable) blob.
const BLOB_MODE: i32 = 0o100644;

pub fn handle_track(
    url: &str,
    id: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<UserAction> {
    let repo = transaction.repo();

    spawn_git_command(repo.path(), &["fetch", url, "HEAD"], &[])?;

    let fetch_head_ref = repo
        .find_reference("FETCH_HEAD")
        .context("Failed to find FETCH_HEAD")?;
    let fetched_commit = fetch_head_ref
        .peel_to_commit()
        .context("Failed to peel FETCH_HEAD to commit")?;

    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let signature = make_signature(repo)?;

    // 1) Filter the fetched remote history under `remotes/<id>/contents`,
    //    preserving its commit history (kept reachable as a merge parent below).
    let prefix_filter = josh_core::filter::parse(&layout::contents_prefix_filter_spec(id))
        .context("Failed to parse prefix filter")?;
    let prefixed_oid =
        josh_core::filter::apply_to_commit(prefix_filter, &fetched_commit, transaction)
            .context("Failed to prefix remote contents")?;
    let prefixed_commit = repo
        .find_commit(prefixed_oid)
        .context("Failed to find prefixed commit")?;
    let prefixed_tree = prefixed_commit
        .tree()
        .context("Failed to get prefixed tree")?;
    trace_commit(repo, prefixed_oid, "contents import");

    // 2) Overlay the prefixed contents onto the current metarepo tree, then add
    //    the workspace.josh and remote.json metadata files.
    let overlaid_oid =
        josh_core::filter::tree::overlay(transaction, head_tree.id(), prefixed_tree.id())
            .context("Failed to overlay contents")?;
    let overlaid_tree = repo
        .find_tree(overlaid_oid)
        .context("Failed to find overlaid tree")?;

    let workspace_blob = repo
        .blob(layout::workspace_josh_content(id).as_bytes())
        .context("Failed to write workspace.josh blob")?;
    let with_ws = josh_core::filter::tree::insert(
        repo,
        &overlaid_tree,
        Path::new(&layout::workspace_josh_path(id)),
        workspace_blob,
        BLOB_MODE,
    )
    .context("Failed to insert workspace.josh")?;

    let meta = RemoteMeta {
        url: url.to_string(),
        ref_: "HEAD".to_string(),
    };
    let meta_blob = repo
        .blob(serde_json::to_vec_pretty(&meta)?.as_slice())
        .context("Failed to write remote.json blob")?;
    let final_tree = josh_core::filter::tree::insert(
        repo,
        &with_ws,
        Path::new(&layout::remote_meta_path(id)),
        meta_blob,
        BLOB_MODE,
    )
    .context("Failed to insert remote.json")?;

    // 3) Commit as a merge of the current metarepo HEAD and the imported
    //    remote history, so the remote's commits stay reachable.
    let commit = repo
        .commit(
            None,
            &signature,
            &signature,
            &format!("Track remote: {}", id),
            &final_tree,
            &[&head_commit, &prefixed_commit],
        )
        .context("Failed to create track commit")?;
    trace_commit(repo, commit, "track merge");

    repo.head()?
        .set_target(commit, "josh-cq track")
        .context("Failed to update HEAD")?;

    let action = UserAction::Message(format!("Tracked remote '{}' at {}", id, url));

    Ok(action)
}
