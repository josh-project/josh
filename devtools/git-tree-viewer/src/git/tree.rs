use git2::{ObjectType, Oid, Repository};

#[derive(Clone)]
pub enum TreeItem {
    Directory {
        name: String,
        oid: Oid,
        children: Vec<TreeItem>,
    },
    File {
        name: String,
        full_path: String,
        oid: Oid,
    },
    Other {
        name: String,
        oid: Oid,
    },
}

pub fn build_tree(repo: &Repository, tree_oid: Oid, path_prefix: &str) -> Vec<TreeItem> {
    let mut items = Vec::new();

    let tree = match repo.find_tree(tree_oid) {
        Ok(tree) => tree,
        Err(_) => return items,
    };

    let mut entries = tree.iter().collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        let a_type = a.kind().unwrap_or(ObjectType::Any);
        let b_type = b.kind().unwrap_or(ObjectType::Any);

        let a_is_tree = a_type == ObjectType::Tree;
        let b_is_tree = b_type == ObjectType::Tree;

        if a_is_tree != b_is_tree {
            return b_is_tree.cmp(&a_is_tree);
        }

        let a_name = a.name().unwrap_or("");
        let b_name = b.name().unwrap_or("");
        a_name.cmp(b_name)
    });

    for entry in entries {
        let entry_type = entry.kind().unwrap_or(ObjectType::Any);
        let entry_name = entry.name().unwrap_or("<invalid>").to_string();
        let oid = entry.id();
        let full_path = if path_prefix.is_empty() {
            entry_name.clone()
        } else {
            format!("{}/{}", path_prefix, entry_name)
        };

        match entry_type {
            ObjectType::Tree => {
                let children = build_tree(repo, oid, &full_path);
                items.push(TreeItem::Directory {
                    name: entry_name,
                    oid,
                    children,
                });
            }
            ObjectType::Blob => {
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
