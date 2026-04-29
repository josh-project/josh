use anyhow::Context;

/// Produce a tar archive (as bytes) from a git tree, using git2 to walk the tree.
/// Works with in-memory objects (e.g. mempack backend) as well as on-disk objects.
pub fn tree_to_tar(repo: &git2::Repository, tree_oid: git2::Oid) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut buf);
        let tree = repo
            .find_tree(tree_oid)
            .with_context(|| format!("tree not found: {tree_oid}"))?;
        append_tree(repo, &tree, "", &mut builder)?;
        builder.finish()?;
    }
    Ok(buf)
}

fn append_tree(
    repo: &git2::Repository,
    tree: &git2::Tree,
    prefix: &str,
    builder: &mut tar::Builder<impl std::io::Write>,
) -> anyhow::Result<()> {
    for entry in tree {
        let name = entry.name().unwrap_or("");
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let filemode = entry.filemode();

        if filemode == 0o120000 {
            // Symlink: stored as blob, content is the link target
            let blob = repo.find_blob(entry.id())?;
            let target =
                std::str::from_utf8(blob.content()).context("symlink target is not valid UTF-8")?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_path(&path)?;
            header.set_link_name(target)?;
            header.set_size(0);
            header.set_cksum();
            builder.append(&header, std::io::empty())?;
        } else if let Some(git2::ObjectType::Tree) = entry.kind() {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_path(format!("{path}/"))?;
            header.set_mode(0o755);
            header.set_size(0);
            header.set_cksum();
            builder.append(&header, std::io::empty())?;

            let subtree = repo.find_tree(entry.id())?;
            append_tree(repo, &subtree, &path, builder)?;
        } else if let Some(git2::ObjectType::Blob) = entry.kind() {
            let blob = repo.find_blob(entry.id())?;
            let is_exec = filemode & 0o111 != 0;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_path(&path)?;
            header.set_mode(if is_exec { 0o755 } else { 0o644 });
            header.set_size(blob.content().len() as u64);
            header.set_cksum();
            builder.append(&header, blob.content())?;
        }
        // Skip other types (submodules etc.)
    }
    Ok(())
}
