use anyhow::Context;
use anyhow::anyhow;
use josh_core::filter::tree;

use std::collections::HashSet;
use std::path::PathBuf;

/// Prepared link addition, ready to be finalized
pub struct PreparedLinkAdd {
    tree_oid: git2::Oid,
    path: PathBuf,
}

impl PreparedLinkAdd {
    pub fn into_commit(
        self,
        transaction: &josh_core::cache::Transaction,
        head_commit: &git2::Commit,
        signature: &git2::Signature,
    ) -> anyhow::Result<git2::Oid> {
        let repo = transaction.repo();
        let tree = repo
            .find_tree(self.tree_oid)
            .context("Failed to find tree")?;

        repo.commit(
            None,
            signature,
            signature,
            &format!("Add link: {}", self.path.display()),
            &tree,
            &[head_commit],
        )
        .context("Failed to create commit")
    }

    /// Get tree OID for custom commit creation
    ///
    /// This is used by josh-cq to add additional files before creating a commit
    pub fn into_tree_oid(self) -> git2::Oid {
        self.tree_oid
    }
}

/// Result from updating links
pub struct UpdateLinksResult {
    /// Commit with updated .link.josh files
    pub commit_with_updates: git2::Oid,
    /// Commit after applying :link filter
    pub filtered_commit: git2::Oid,
}

/// A remote URL and commit SHA found in a `.link.josh` file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinkRef {
    pub remote: String,
    pub commit: String,
}

/// Walk the entire commit history reachable from the given commit and collect
/// all (remote, commit) pairs found in any `.link.josh` file across all commits and trees.
pub fn collect_all_link_refs(
    transaction: &josh_core::cache::Transaction,
    commit: git2::Oid,
) -> anyhow::Result<HashSet<LinkRef>> {
    let repo = transaction.repo();

    // Apply a filter that keeps only .link.josh files. This prunes the history
    // to only commits that actually changed those files, so the revwalk below
    // visits far fewer commits on typical repositories.
    let link_file_filter =
        josh_core::filter::parse("::**/.link.josh").context("Failed to parse .link.josh filter")?;

    let filtered_commit = josh_core::filter_commit(transaction, link_file_filter, commit)
        .context("Failed to apply .link.josh filter")?;

    if filtered_commit == git2::Oid::zero() {
        return Ok(HashSet::new());
    }

    let mut refs = HashSet::new();

    let mut walk = repo.revwalk().context("Failed to create revwalk")?;
    walk.push(filtered_commit)
        .context("Failed to push commit to revwalk")?;

    for oid in walk {
        let oid = oid.context("Failed to get OID from revwalk")?;
        let commit = repo.find_commit(oid).context("Failed to find commit")?;
        let tree = commit.tree().context("Failed to get commit tree")?;

        let link_files =
            josh_core::link::find_link_files(repo, &tree).context("Failed to find link files")?;

        for (_, filter) in link_files {
            if let (Some(remote), Some(commit)) =
                (filter.get_meta("remote"), filter.get_meta("commit"))
            {
                refs.insert(LinkRef { remote, commit });
            }
        }
    }

    Ok(refs)
}

pub fn make_signature(repo: &git2::Repository) -> anyhow::Result<git2::Signature<'static>> {
    if let Ok(time) = std::env::var("JOSH_COMMIT_TIME") {
        git2::Signature::new(
            "JOSH",
            "josh@josh-project.dev",
            &git2::Time::new(time.parse().context("Failed to parse JOSH_COMMIT_TIME")?, 0),
        )
        .context("Failed to create signature")
    } else {
        let sig = repo.signature().context("Failed to get signature")?;
        Ok(sig.to_owned())
    }
}

/// Prepare a link addition without creating a commit
pub fn prepare_link_add(
    transaction: &josh_core::cache::Transaction,
    path: &std::path::Path,
    url: &str,
    filter: Option<&str>,
    target: &str,
    fetched_commit: git2::Oid,
    head_tree: &git2::Tree,
    mode: josh_core::filter::LinkMode,
) -> anyhow::Result<PreparedLinkAdd> {
    let repo = transaction.repo();

    // Strip leading slash if present (git tree paths are always relative)
    let path = path.strip_prefix("/").unwrap_or(path);
    let filter = filter.unwrap_or(":/");

    // Parse the filter
    let filter_obj = josh_core::filter::parse(filter)
        .with_context(|| format!("Failed to parse filter '{}'", filter))?;

    // Create a filter with metadata
    let link_filter = filter_obj
        .with_meta("remote", url.to_string())
        .with_meta("target", target.to_string())
        .with_meta("commit", fetched_commit.to_string())
        .with_meta("mode", mode.to_string());
    let link_content = josh_core::filter::as_file(link_filter, 0);

    // Create the blob for the .link.josh file
    let link_blob = repo
        .blob(link_content.as_bytes())
        .context("Failed to create blob")?;

    // Create the path for the .link.josh file
    let link_path = path.join(".link.josh");

    // Insert the .link.josh file into the tree
    let new_tree = tree::insert(
        repo,
        head_tree,
        &link_path,
        link_blob,
        git2::FileMode::Blob.into(),
    )
    .context("Failed to insert link file into tree")?;

    Ok(PreparedLinkAdd {
        tree_oid: new_tree.id(),
        path: path.to_path_buf(),
    })
}

pub fn update_links(
    repo: &git2::Repository,
    transaction: &josh_core::cache::Transaction,
    head_commit: &git2::Commit,
    links_to_update: Vec<(PathBuf, git2::Oid)>,
    signature: &git2::Signature,
) -> anyhow::Result<Option<UpdateLinksResult>> {
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;
    let head_tree_id = head_tree.id();

    // Find all link files to get their current metadata
    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    // Update the link files with new commit OIDs
    let mut updated_link_files: Vec<(PathBuf, josh_core::filter::Filter)> = Vec::new();
    for (path, new_oid) in &links_to_update {
        // Find the existing link file at this path
        let link_file = link_files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, lf)| lf)
            .ok_or_else(|| anyhow!("Link file not found at path '{}'", path.display()))?;

        // Update the link file with the new commit SHA
        let updated_link_file = link_file.with_meta("commit", new_oid.to_string());
        updated_link_files.push((path.clone(), updated_link_file));
    }

    // Create new tree with updated .link.josh files
    let mut new_tree = head_tree;
    for (path, link_file) in &updated_link_files {
        let link_content = josh_core::filter::as_file(*link_file, 0);

        // Create the blob for the .link.josh file
        let link_blob = repo
            .blob(link_content.as_bytes())
            .context("Failed to create blob")?;

        // Create the path for the .link.josh file
        let link_path = path.join(".link.josh");

        // Insert the updated .link.josh file into the tree
        new_tree =
            tree::insert(repo, &new_tree, &link_path, link_blob, 0o0100644).with_context(|| {
                format!(
                    "Failed to insert link file into tree at path '{}'",
                    path.display()
                )
            })?;
    }

    if new_tree.id() == head_tree_id {
        return Ok(None);
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
    let link_filter = josh_core::filter::parse(":link").context("Failed to parse :link filter")?;

    let filtered_commit = josh_core::filter_commit(transaction, link_filter, commit_with_updates)
        .context("Failed to apply :link filter")?;

    Ok(Some(UpdateLinksResult {
        commit_with_updates,
        filtered_commit,
    }))
}
