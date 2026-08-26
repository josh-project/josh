pub mod blob;
pub mod repo;
pub mod tree;

pub use blob::load_blob_content;
pub use repo::{open_repo, resolve_commit};
pub use tree::{build_tree, TreeItem};

#[cfg(test)]
mod tests {
    use super::{build_tree, load_blob_content, open_repo, resolve_commit, TreeItem};

    #[test]
    fn opens_repository_and_loads_commit_tree() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let repo = gix::init(dir.path())?;
        let blob_id = repo.write_blob(b"hello from gitoxide")?.detach();
        let tree_id = repo
            .write_object(gix::objs::Tree {
                entries: vec![gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Blob.into(),
                    filename: "hello.txt".into(),
                    oid: blob_id,
                }],
            })?
            .detach();
        let signature = gix::actor::Signature {
            name: "Josh Test".into(),
            email: "josh@example.com".into(),
            time: gix::actor::date::Time {
                seconds: 0,
                offset: 0,
            },
        };
        let mut committer_time = gix::date::parse::TimeBuf::default();
        let mut author_time = gix::date::parse::TimeBuf::default();
        repo.commit_as(
            signature.to_ref(&mut committer_time),
            signature.to_ref(&mut author_time),
            "HEAD",
            "initial",
            tree_id,
            std::iter::empty::<gix::ObjectId>(),
        )?;

        std::fs::create_dir(dir.path().join("nested"))?;
        let discovered = open_repo(dir.path().join("nested"))?;
        let commit_id = resolve_commit(&discovered, None)?;
        let commit = discovered.find_commit(commit_id)?;
        let items = build_tree(&discovered, commit.tree_id()?.detach(), "");

        match items.as_slice() {
            [TreeItem::File {
                name,
                full_path,
                oid,
            }] => {
                assert_eq!(name, "hello.txt");
                assert_eq!(full_path, "hello.txt");
                assert_eq!(load_blob_content(&discovered, *oid), "hello from gitoxide");
            }
            _ => panic!("unexpected tree contents"),
        }

        Ok(())
    }
}
