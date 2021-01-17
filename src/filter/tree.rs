use super::*;

pub fn dirtree<'a>(
    repo: &'a git2::Repository,
    root: &str,
    input: git2::Oid,
    cache: &mut std::collections::HashMap<(git2::Oid, String), git2::Oid>,
) -> super::JoshResult<git2::Tree<'a>> {
    if let Some(cached) = cache.get(&(input, root.to_string())) {
        return Ok(repo.find_tree(*cached)?);
    }

    let tree = repo.find_tree(input)?;
    let mut result = tree::empty(&repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(super::josh_error("INVALID_FILENAME"))?;

        if entry.kind() == Some(git2::ObjectType::Blob) {
            if name == "workspace.josh" {
                result = replace_child(
                    &repo,
                    &std::path::Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    entry.id(),
                    &result,
                )?;
            }
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = dirtree(
                &repo,
                &format!(
                    "{}{}{}",
                    root,
                    if root == "" { "" } else { "/" },
                    entry.name().ok_or(super::josh_error("no name"))?
                ),
                entry.id(),
                cache,
            )?
            .id();

            if s != tree::empty_id() {
                result = replace_child(
                    &repo,
                    &std::path::Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    s,
                    &result,
                )?;
            }
        }
    }

    if root != "" {
        let empty_blob = repo.blob("".as_bytes())?;

        result = replace_child(
            &repo,
            &std::path::Path::new(&format!(
                "JOSH_ORIG_PATH_{}",
                super::to_ns(&root)
            )),
            empty_blob,
            &result,
        )?;
    }
    cache.insert((input, root.to_string()), result.id());
    return Ok(result);
}

pub fn remove_pred<'a>(
    repo: &'a git2::Repository,
    root: &str,
    input: git2::Oid,
    pred: &dyn Fn(&std::path::Path, bool) -> bool,
    key: git2::Oid,
    cache: &mut std::collections::HashMap<(git2::Oid, git2::Oid), git2::Oid>,
) -> super::JoshResult<git2::Tree<'a>> {
    if let Some(cached) = cache.get(&(input, key)) {
        return Ok(repo.find_tree(*cached)?);
    }
    rs_tracing::trace_scoped!("remove_pred X", "root": root);

    let tree = repo.find_tree(input)?;
    let mut result = tree::empty(&repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(super::josh_error("INVALID_FILENAME"))?;
        let path = std::path::PathBuf::from(root).join(name);

        if entry.kind() == Some(git2::ObjectType::Blob) {
            if pred(&path, true) {
                result = replace_child(
                    &repo,
                    &std::path::Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    entry.id(),
                    &result,
                )?;
            }
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = if (root != "") && pred(&path, false) {
                entry.id()
            } else {
                remove_pred(
                    &repo,
                    &format!(
                        "{}{}{}",
                        root,
                        if root == "" { "" } else { "/" },
                        entry.name().ok_or(super::josh_error("no name"))?
                    ),
                    entry.id(),
                    &pred,
                    key,
                    cache,
                )?
                .id()
            };

            if s != tree::empty_id() {
                result = replace_child(
                    &repo,
                    &std::path::Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    s,
                    &result,
                )?;
            }
        }
    }

    cache.insert((input, key), result.id());
    return Ok(result);
}

pub fn subtract(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    if input1 == input2 {
        return Ok(tree::empty_id());
    }
    if input1 == tree::empty_id() {
        return Ok(tree::empty_id());
    }

    if let (Ok(tree1), Ok(tree2)) =
        (repo.find_tree(input1), repo.find_tree(input2))
    {
        if input2 == tree::empty_id() {
            return Ok(input1);
        }
        rs_tracing::trace_scoped!("subtract fast");
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1
                .get_name(entry.name().ok_or(super::josh_error("no name"))?)
            {
                result_tree = replace_child(
                    &repo,
                    &std::path::Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    subtract(repo, e.id(), entry.id())?,
                    &result_tree,
                )?;
            }
        }

        return Ok(result_tree.id());
    }

    return Ok(tree::empty_id());
}

fn replace_child<'a>(
    repo: &'a git2::Repository,
    child: &std::path::Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> super::JoshResult<git2::Tree<'a>> {
    let mode = if let Ok(_) = repo.find_tree(oid) {
        0o0040000 // GIT_FILEMODE_TREE
    } else {
        0o0100644
    };

    let full_tree_id = {
        let mut builder = repo.treebuilder(Some(&full_tree))?;
        if oid == git2::Oid::zero() {
            builder.remove(child).ok();
        } else if oid == tree::empty_id() {
            builder.remove(child).ok();
        } else {
            builder.insert(child, oid, mode).ok();
        }
        builder.write()?
    };
    return Ok(repo.find_tree(full_tree_id)?);
}

pub fn insert<'a>(
    repo: &'a git2::Repository,
    full_tree: &git2::Tree,
    path: &std::path::Path,
    oid: git2::Oid,
) -> super::JoshResult<git2::Tree<'a>> {
    if path.components().count() == 1 {
        return replace_child(&repo, path, oid, full_tree);
    } else {
        let name = std::path::Path::new(
            path.file_name().ok_or(super::josh_error("file_name"))?,
        );
        let path = path.parent().ok_or(super::josh_error("path.parent"))?;

        let st = if let Ok(st) = full_tree.get_path(path) {
            repo.find_tree(st.id()).unwrap_or(tree::empty(&repo))
        } else {
            tree::empty(&repo)
        };

        let tree = replace_child(&repo, name, oid, &st)?;

        return insert(&repo, full_tree, path, tree.id());
    }
}

pub fn overlay(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("overlay");
    if input1 == input2 {
        return Ok(input1);
    }
    if input1 == tree::empty_id() {
        return Ok(input2);
    }
    if input2 == tree::empty_id() {
        return Ok(input1);
    }

    if let (Ok(tree1), Ok(tree2)) =
        (repo.find_tree(input1), repo.find_tree(input2))
    {
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1
                .get_name(entry.name().ok_or(super::josh_error("no name"))?)
            {
                result_tree = replace_child(
                    &repo,
                    &std::path::Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    overlay(repo, entry.id(), e.id())?,
                    &result_tree,
                )?;
            } else {
                result_tree = replace_child(
                    &repo,
                    &std::path::Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    entry.id(),
                    &result_tree,
                )?;
            }
        }

        return Ok(result_tree.id());
    }

    return Ok(input1);
}

pub fn compose<'a>(
    repo: &'a git2::Repository,
    trees: Vec<(&super::filter::Filter, git2::Tree<'a>)>,
) -> super::JoshResult<git2::Tree<'a>> {
    rs_tracing::trace_scoped!("compose");
    let mut result = tree::empty(&repo);
    let mut taken = tree::empty(&repo);
    for (f, applied) in trees {
        let taken_applied = super::filter::apply(&repo, *f, taken.clone())?;
        let subtracted =
            repo.find_tree(subtract(&repo, applied.id(), taken_applied.id())?)?;
        taken = super::filter::unapply(&repo, *f, applied, taken.clone())?;
        result =
            repo.find_tree(overlay(&repo, result.id(), subtracted.id())?)?;
    }

    Ok(result)
}

pub fn get_blob(
    repo: &git2::Repository,
    tree: &git2::Tree,
    path: &Path,
) -> String {
    let entry_oid = ok_or!(tree.get_path(&path).map(|x| x.id()), {
        return "".to_owned();
    });

    let blob = ok_or!(repo.find_blob(entry_oid), {
        return "".to_owned();
    });

    let content = ok_or!(std::str::from_utf8(blob.content()), {
        return "".to_owned();
    });

    return content.to_owned();
}

pub fn empty_id() -> git2::Oid {
    return git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")
        .unwrap();
}

pub fn empty(repo: &git2::Repository) -> git2::Tree {
    repo.find_tree(empty_id()).unwrap()
}
