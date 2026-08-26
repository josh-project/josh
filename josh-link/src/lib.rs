use anyhow::Context;
use anyhow::anyhow;
use josh_core::filter::tree;

use std::collections::HashSet;
use std::path::PathBuf;

/// Prepared link addition, ready to be finalized
pub struct PreparedLinkAdd {
    tree_oid: gix_hash::ObjectId,
    path: PathBuf,
}

impl PreparedLinkAdd {
    pub fn into_commit(
        self,
        transaction: &josh_core::cache::Transaction,
        head_commit: gix_hash::ObjectId,
        signature: &gix_actor::Signature,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        josh_core::objects::write_commit(
            transaction.odb(),
            self.tree_oid,
            &[head_commit],
            signature,
            signature,
            &format!("Add link: {}", self.path.display()),
        )
        .context("Failed to create commit")
    }

    /// Get tree OID for custom commit creation
    ///
    /// This is used by josh-cq to add additional files before creating a commit
    pub fn into_tree_oid(self) -> gix_hash::ObjectId {
        self.tree_oid
    }
}

/// Result from updating links
pub struct UpdateLinksResult {
    /// Commit with updated .link.josh files
    pub commit_with_updates: gix_hash::ObjectId,
    /// Commit after applying :link filter
    pub filtered_commit: gix_hash::ObjectId,
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
    commit: gix_hash::ObjectId,
) -> anyhow::Result<HashSet<LinkRef>> {
    // Apply a filter that keeps only .link.josh files. This prunes the history
    // to only commits that actually changed those files, so the revwalk below
    // visits far fewer commits on typical repositories.
    let link_file_filter =
        josh_core::filter::parse("::**/.link.josh").context("Failed to parse .link.josh filter")?;

    let filtered_commit = josh_core::filter_commit(transaction, link_file_filter, commit)
        .context("Failed to apply .link.josh filter")?;

    if filtered_commit == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
        return Ok(HashSet::new());
    }

    let mut refs = HashSet::new();

    let odb = transaction.odb();
    let mut walk = josh_core::objects::RevWalk::new(odb);
    walk.push(filtered_commit)
        .context("Failed to push commit to revwalk")?;

    for oid in walk
        .into_topo_vec(|_| false)
        .context("Failed to walk history")?
    {
        let tree = josh_core::git::read_tree_id(odb, oid).context("Failed to get commit tree")?;

        let link_files =
            josh_core::link::find_link_files(odb, tree).context("Failed to find link files")?;

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

pub fn make_signature(
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<gix_actor::Signature> {
    if let Ok(time) = std::env::var("JOSH_COMMIT_TIME") {
        Ok(gix_actor::Signature {
            name: "JOSH".into(),
            email: "josh@josh-project.dev".into(),
            time: gix_actor::date::Time {
                seconds: time.parse().context("Failed to parse JOSH_COMMIT_TIME")?,
                offset: 0,
            },
        })
    } else {
        transaction.signature().context("Failed to get signature")
    }
}

/// Prepare a link addition without creating a commit
pub fn prepare_link_add(
    transaction: &josh_core::cache::Transaction,
    path: &std::path::Path,
    url: &str,
    filter: Option<&str>,
    target: &str,
    fetched_commit: gix_hash::ObjectId,
    head_tree: gix_hash::ObjectId,
    mode: josh_core::filter::LinkMode,
) -> anyhow::Result<PreparedLinkAdd> {
    let odb = transaction.odb();

    // Strip leading slash if present (git tree paths are always relative)
    let path = path.strip_prefix("/").unwrap_or(path);
    let filter = filter.unwrap_or(":/");

    // Parse the filter
    let filter_obj = josh_core::filter::parse(filter)
        .with_context(|| format!("Failed to parse filter '{}'", filter))?;

    let filter_obj = filter_obj.prefix(&path);

    // Create a filter with metadata
    let link_filter = filter_obj
        .with_meta("remote", url.to_string())
        .with_meta("target", target.to_string())
        .with_meta("commit", fetched_commit.to_string())
        .with_meta("mode", mode.to_string());
    let link_content = josh_core::filter::as_file(link_filter, 0);

    let link_blob = josh_core::objects::write_blob(odb, link_content.as_bytes())?;
    let link_path = path.join(".link.josh");

    let new_tree = tree::insert_oid(odb, head_tree, &link_path, link_blob, 0o0100644)
        .context("Failed to insert link file into tree")?;

    Ok(PreparedLinkAdd {
        tree_oid: new_tree,
        path: path.to_path_buf(),
    })
}

pub fn update_links(
    transaction: &josh_core::cache::Transaction,
    head_commit: gix_hash::ObjectId,
    links_to_update: Vec<(PathBuf, gix_hash::ObjectId)>,
    signature: &gix_actor::Signature,
) -> anyhow::Result<Option<UpdateLinksResult>> {
    let odb = transaction.odb();
    let head_tree_id =
        josh_core::git::read_tree_id(odb, head_commit).context("Failed to get HEAD tree")?;

    // Find all link files to get their current metadata
    let link_files =
        josh_core::link::find_link_files(odb, head_tree_id).context("Failed to find link files")?;

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
    let mut new_tree = head_tree_id;
    for (path, link_file) in &updated_link_files {
        let link_content = josh_core::filter::as_file(*link_file, 0);
        let link_blob = josh_core::objects::write_blob(odb, link_content.as_bytes())?;
        let link_path = path.join(".link.josh");

        new_tree = tree::insert_oid(odb, new_tree, &link_path, link_blob, 0o0100644).with_context(
            || {
                format!(
                    "Failed to insert link file into tree at path '{}'",
                    path.display()
                )
            },
        )?;
    }

    if new_tree == head_tree_id {
        return Ok(None);
    }

    // Create a new commit with the updated tree
    let commit_with_updates = josh_core::objects::write_commit(
        odb,
        new_tree,
        &[head_commit],
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
