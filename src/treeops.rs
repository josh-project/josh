use git2::*;
use std::path::Path;

fn get_subtree(tree: &Tree, path: &Path) -> Option<Oid>
{
    tree.get_path(path).map(|x| x.id()).ok()
}

pub fn find_all_subdirs(repo: &Repository, tree: &Tree) -> Vec<String>
{
    let mut sd = vec![];
    for item in tree {
        if let Ok(st) = repo.find_tree(item.id()) {
            let name = item.name().unwrap();
            if !name.starts_with(".") {
                sd.push(name.to_string());
                for r in find_all_subdirs(&repo, &st) {
                    sd.push(format!("{}/{}", name, r));
                }
            }
        }
    }
    return sd;
}


fn replace_child(repo: &Repository, child: &Path, subtree: Oid, full_tree: &Tree) -> Oid
{
    let full_tree_id = {
        let mut builder = repo.treebuilder(Some(&full_tree))
            .expect("replace_child: can't create treebuilder");
        builder.insert(child, subtree, 0o0040000) // GIT_FILEMODE_TREE
            .expect("replace_child: can't insert tree");
        builder.write().expect("replace_child: can't write tree")
    };
    return full_tree_id;
}

pub fn replace_subtree(repo: &Repository, path: &Path, subtree: &Tree, full_tree: &Tree) -> Oid
{
    if path.components().count() == 1 {
        return repo.find_tree(replace_child(&repo, path, subtree.id(), full_tree))
            .expect("replace_child: can't find new tree")
            .id();
    } else {
        let name = Path::new(path.file_name().expect("no module name"));
        let path = path.parent().expect("module not in subdir");

        let st = if let Some(st) = get_subtree(&full_tree, path) {
            repo.find_tree(st).unwrap()
        } else {
            let empty = repo.treebuilder(None).unwrap().write().unwrap();
            repo.find_tree(empty).unwrap()
        };

        let tree = repo.find_tree(replace_child(&repo, name, subtree.id(), &st))
            .expect("replace_child: can't find new tree");

        return replace_subtree(&repo, path, &tree, full_tree);
    }
}
