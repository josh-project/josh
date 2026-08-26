#[derive(Clone)]
pub enum TreeItem {
    Directory {
        name: String,
        oid: gix::ObjectId,
        children: Vec<TreeItem>,
    },
    File {
        name: String,
        full_path: String,
        oid: gix::ObjectId,
    },
    Other {
        name: String,
        oid: gix::ObjectId,
    },
}

pub fn build_tree(
    repo: &gix::Repository,
    tree_oid: gix::ObjectId,
    path_prefix: &str,
) -> Vec<TreeItem> {
    let mut items = Vec::new();

    let tree = match repo.find_tree(tree_oid) {
        Ok(tree) => tree,
        Err(_) => return items,
    };

    let mut entries = match tree.iter().collect::<Result<Vec<_>, _>>() {
        Ok(entries) => entries,
        Err(_) => return items,
    };
    entries.sort_by(|a, b| {
        let a_is_tree = a.kind() == gix::object::tree::EntryKind::Tree;
        let b_is_tree = b.kind() == gix::object::tree::EntryKind::Tree;

        if a_is_tree != b_is_tree {
            return b_is_tree.cmp(&a_is_tree);
        }

        std::str::from_utf8(a.filename())
            .unwrap_or("")
            .cmp(std::str::from_utf8(b.filename()).unwrap_or(""))
    });

    for entry in entries {
        let entry_type = entry.kind();
        let entry_name = std::str::from_utf8(entry.filename())
            .unwrap_or("<invalid>")
            .to_string();
        let oid = entry.id().detach();
        let full_path = if path_prefix.is_empty() {
            entry_name.clone()
        } else {
            format!("{path_prefix}/{entry_name}")
        };

        match entry_type {
            gix::object::tree::EntryKind::Tree => {
                let children = build_tree(repo, oid, &full_path);
                items.push(TreeItem::Directory {
                    name: entry_name,
                    oid,
                    children,
                });
            }
            gix::object::tree::EntryKind::Blob | gix::object::tree::EntryKind::BlobExecutable => {
                items.push(TreeItem::File {
                    name: entry_name,
                    full_path,
                    oid,
                });
            }
            _ => {
                items.push(TreeItem::Other {
                    name: entry_name,
                    oid,
                });
            }
        }
    }

    items
}
