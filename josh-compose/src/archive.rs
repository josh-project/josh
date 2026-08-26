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
            header.set_size(0);
            builder.append_link(&mut header, &path, target)?;
        } else if entry.mode.is_tree() {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            builder.append_data(&mut header, format!("{path}/"), std::io::empty())?;

            append_tree(transaction, odb, id, &path, builder)?;
        } else if entry.mode.is_blob() {
            let content =
                tree::blob_bytes(odb, id).with_context(|| format!("blob not found: {path}"))?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(if entry.mode.is_executable() {
                0o755
            } else {
                0o644
            });
            header.set_size(content.len() as u64);
            builder.append_data(&mut header, &path, &content[..])?;
        }
        // Skip other types (submodules etc.)
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_transaction(td: &tempfile::TempDir) -> anyhow::Result<cache::Transaction> {
        let cache = cache::CacheStack::new().with_backend(cache::SledCacheBackend::new(td.path()));
        Ok(cache::TransactionContext::new(td.path(), cache.into()).open()?)
    }

    #[test]
    fn round_trips_long_paths_and_symlink_targets() -> anyhow::Result<()> {
        let td = tempfile::tempdir()?;
        gix::init_bare(td.path())?;
        let deep_directory = format!("source/{}/{}", "a".repeat(50), "b".repeat(50));
        let file_path = format!("{deep_directory}/file.txt");
        let link_path = format!("links/{}/link", "c".repeat(96));
        let link_target = format!("target/{}", "d".repeat(100));
        assert!(file_path.len() > 100);
        assert!(deep_directory.len() > 100);
        assert!(link_path.len() > 100);
        assert!(link_target.len() > 100);

        let file_content = b"long path content\n";
        let transaction = open_transaction(&td)?;
        let odb = transaction.odb();
        let file_oid = josh_core::objects::write_blob(odb, file_content)?;
        let link_oid = josh_core::objects::write_blob(odb, link_target.as_bytes())?;
        let tree_oid = tree::insert_oid(
            odb,
            gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1),
            std::path::Path::new(&file_path),
            file_oid,
            0o100644,
        )?;
        let tree_oid = tree::insert_oid(
            odb,
            tree_oid,
            std::path::Path::new(&link_path),
            link_oid,
            0o120000,
        )?;

        let archive = tree_to_tar(&transaction, odb, tree_oid)?;
        let extracted = tempfile::tempdir()?;
        tar::Archive::new(archive.as_slice()).unpack(extracted.path())?;

        assert_eq!(
            std::fs::read(extracted.path().join(&file_path))?,
            file_content
        );
        assert!(extracted.path().join(&deep_directory).is_dir());
        let extracted_link = extracted.path().join(&link_path);
        assert!(
            std::fs::symlink_metadata(&extracted_link)?
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(extracted_link)?,
            std::path::Path::new(&link_target)
        );
        Ok(())
    }
}
