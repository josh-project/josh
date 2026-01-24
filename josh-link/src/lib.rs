use anyhow::Context;
use josh_core::filter::tree;
use std::path::PathBuf;

/// Result from adding a link to a repository
pub struct AddLinkResult {
    /// Commit with .link.josh file added
    pub commit_with_link: git2::Oid,
    /// Commit after applying :link=snapshot filter
    pub filtered_commit: git2::Oid,
}

/// Result from updating links
pub struct UpdateLinksResult {
    /// Commit with updated .link.josh files
    pub commit_with_updates: git2::Oid,
    /// Commit after applying :link filter
    pub filtered_commit: git2::Oid,
}

pub fn from_josh_err(e: josh_core::JoshError) -> anyhow::Error {
    anyhow::anyhow!("{}", e.0)
}

pub fn add_link(
    transaction: &josh_core::cache::Transaction,
    path: &str,
    url: &str,
    filter: Option<&str>,
    target: &str,
    fetched_commit: git2::Oid,
    head_commit: &git2::Commit,
    signature: &git2::Signature,
) -> anyhow::Result<AddLinkResult> {
    let repo = transaction.repo();

    // Normalize the path by removing leading and trailing slashes
    let normalized_path = path.trim_matches('/').to_string();

    // Get the filter (default to ":/" if not provided)
    let filter_str = filter.unwrap_or(":/");

    // Parse the filter
    let filter_obj = josh_core::filter::parse(filter_str)
        .map_err(from_josh_err)
        .with_context(|| format!("Failed to parse filter '{}'", filter_str))?;

    // Create a filter with metadata
    let link_filter = filter_obj
        .with_meta("remote", url.to_string())
        .with_meta("target", target.to_string())
        .with_meta("commit", fetched_commit.to_string());
    let link_content = josh_core::filter::pretty(link_filter, 0);

    // Create the blob for the .link.josh file
    let link_blob = repo
        .blob(link_content.as_bytes())
        .context("Failed to create blob")?;

    // Create the path for the .link.josh file
    let link_path = std::path::Path::new(&normalized_path).join(".link.josh");

    // Get the head tree
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Insert the .link.josh file into the tree
    let new_tree = tree::insert(repo, &head_tree, &link_path, link_blob, 0o0100644)
        .map_err(from_josh_err)
        .context("Failed to insert link file into tree")?;

    // Create a new commit with the updated tree
    let commit_with_link = repo
        .commit(
            None, // Don't update any reference
            signature,
            signature,
            &format!("Add link: {}", normalized_path),
            &new_tree,
            &[head_commit],
        )
        .context("Failed to create commit")?;

    // Apply the :link=snapshot filter to the new commit
    let snapshot_filter = josh_core::filter::parse(":link=snapshot")
        .map_err(from_josh_err)
        .context("Failed to parse :link=snapshot filter")?;

    let filtered_commit = josh_core::filter_commit(transaction, snapshot_filter, commit_with_link)
        .map_err(from_josh_err)
        .context("Failed to apply :link=snapshot filter")?;

    Ok(AddLinkResult {
        commit_with_link,
        filtered_commit,
    })
}

pub fn update_links(
    repo: &git2::Repository,
    transaction: &josh_core::cache::Transaction,
    head_commit: &git2::Commit,
    links_to_update: Vec<(PathBuf, git2::Oid)>,
    signature: &git2::Signature,
) -> anyhow::Result<UpdateLinksResult> {
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Find all link files to get their current metadata
    let link_files = josh_core::find_link_files(repo, &head_tree)
        .map_err(from_josh_err)
        .context("Failed to find link files")?;

    // Update the link files with new commit OIDs
    let mut updated_link_files: Vec<(PathBuf, josh_core::filter::Filter)> = Vec::new();
    for (path, new_oid) in &links_to_update {
        // Find the existing link file at this path
        let link_file = link_files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, lf)| lf)
            .ok_or_else(|| anyhow::anyhow!("Link file not found at path '{}'", path.display()))?;

        // Update the link file with the new commit SHA
        let updated_link_file = link_file.with_meta("commit", new_oid.to_string());
        updated_link_files.push((path.clone(), updated_link_file));
    }

    // Create new tree with updated .link.josh files
    let mut new_tree = head_tree;
    for (path, link_file) in &updated_link_files {
        let link_content = josh_core::filter::pretty(*link_file, 0);

        // Create the blob for the .link.josh file
        let link_blob = repo
            .blob(link_content.as_bytes())
            .context("Failed to create blob")?;

        // Create the path for the .link.josh file
        let link_path = path.join(".link.josh");

        // Insert the updated .link.josh file into the tree
        new_tree = tree::insert(repo, &new_tree, &link_path, link_blob, 0o0100644)
            .map_err(from_josh_err)
            .with_context(|| {
                format!(
                    "Failed to insert link file into tree at path '{}'",
                    path.display()
                )
            })?;
    }

    // Create a new commit with the updated tree
    let commit_with_updates = repo
        .commit(
            None, // Don't update any reference
            signature,
            signature,
            &format!(
                "Update links: {}",
                updated_link_files
                    .iter()
                    .map(|(p, _)| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            &new_tree,
            &[head_commit],
        )
        .context("Failed to create commit")?;

    // Apply the :link filter to the new commit
    let link_filter = josh_core::filter::parse(":link")
        .map_err(from_josh_err)
        .context("Failed to parse :link filter")?;

    let filtered_commit = josh_core::filter_commit(transaction, link_filter, commit_with_updates)
        .map_err(from_josh_err)
        .context("Failed to apply :link filter")?;

    Ok(UpdateLinksResult {
        commit_with_updates,
        filtered_commit,
    })
}
