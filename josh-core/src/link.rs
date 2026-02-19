#[cfg(feature = "incubating")]
pub fn find_link_files(
    repo: &git2::Repository,
    tree: &git2::Tree,
) -> anyhow::Result<Vec<(std::path::PathBuf, crate::filter::Filter)>> {
    use crate::filter;
    use anyhow::Context;
    let mut link_files = Vec::new();

    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if let Some(name) = entry.name() {
            if name == ".link.josh" {
                // Found a link file
                let link_blob = match repo.find_blob(entry.id()) {
                    Ok(blob) => blob,
                    Err(e) => {
                        eprintln!("Failed to find blob: {}", e);
                        return git2::TreeWalkResult::Skip;
                    }
                };

                let link_content = match std::str::from_utf8(link_blob.content()) {
                    Ok(content) => content,
                    Err(e) => {
                        eprintln!("Failed to parse link file content: {}", e);
                        return git2::TreeWalkResult::Skip;
                    }
                };

                let filter = match filter::parse(link_content.trim()) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("Failed to parse .link.josh filter: {}", e);
                        return git2::TreeWalkResult::Skip;
                    }
                };

                let root = root.trim_matches('/');
                // Use root as the directory path where the .link.josh file is located
                let path = std::path::PathBuf::from(root);

                link_files.push((path, filter));
            }
        }

        git2::TreeWalkResult::Ok
    })
    .context("Failed to walk tree")?;

    Ok(link_files)
}
