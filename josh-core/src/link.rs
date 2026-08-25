pub fn find_link_files(
    src: &impl gix_object::Find,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<Vec<(std::path::PathBuf, crate::filter::Filter)>> {
    use crate::filter;
    use crate::objects;
    use anyhow::Context;
    let mut link_files = Vec::new();

    objects::walk_tree_preorder(src, tree, &mut |root, entry| {
        if &entry.filename[..] == b".link.josh" {
            let mut buf = Vec::new();
            let link_content = match src.try_find(entry.oid, &mut buf) {
                Ok(Some(data)) if data.kind == gix_object::Kind::Blob => data.data,
                Ok(_) => {
                    eprintln!("Failed to find blob: object {} not found", entry.oid);
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Failed to find blob: {}", e);
                    return Ok(());
                }
            };

            let link_content = match std::str::from_utf8(link_content) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!("Failed to parse link file content: {}", e);
                    return Ok(());
                }
            };

            let filter = match filter::parse(link_content.trim()) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Failed to parse .link.josh filter: {}", e);
                    return Ok(());
                }
            };

            let path = std::path::PathBuf::from(root);

            link_files.push((path, filter));
        }

        Ok(())
    })
    .context("Failed to walk tree")?;

    Ok(link_files)
}
