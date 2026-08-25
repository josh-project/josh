use anyhow::Context;
use josh_core::cache;
use josh_core::filter::tree;
use josh_core::memodb;

/// Produce a tar archive (as bytes) from a git tree.
pub fn tree_to_tar(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    tree_oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut buf);
        append_tree(transaction, odb, tree_oid, "", &mut builder)?;
        builder.finish()?;
    }
    Ok(buf)
}

fn append_tree(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    tree_oid: gix_hash::ObjectId,
    prefix: &str,
    builder: &mut tar::Builder<impl std::io::Write>,
) -> anyhow::Result<()> {
    let reader = tree::read_tree(transaction, odb, tree_oid)?;
    for entry in reader.entries() {
        let name = String::from_utf8_lossy(entry.filename);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let id = entry.oid.to_owned();

        if entry.mode.is_link() {
            let content = tree::blob_bytes(odb, id)
                .with_context(|| format!("symlink target not found: {path}"))?;
            let target =
                std::str::from_utf8(&content).context("symlink target is not valid UTF-8")?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_path(&path)?;
            header.set_link_name(target)?;
            header.set_size(0);
            header.set_cksum();
            builder.append(&header, std::io::empty())?;
        } else if entry.mode.is_tree() {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_path(format!("{path}/"))?;
            header.set_mode(0o755);
            header.set_size(0);
            header.set_cksum();
            builder.append(&header, std::io::empty())?;

            append_tree(transaction, odb, id, &path, builder)?;
        } else if entry.mode.is_blob() {
            let content =
                tree::blob_bytes(odb, id).with_context(|| format!("blob not found: {path}"))?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_path(&path)?;
            header.set_mode(if entry.mode.is_executable() {
                0o755
            } else {
                0o644
            });
            header.set_size(content.len() as u64);
            header.set_cksum();
            builder.append(&header, &content[..])?;
        }
        // Skip other types (submodules etc.)
    }
    Ok(())
}
