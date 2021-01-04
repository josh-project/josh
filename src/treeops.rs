use super::*;

pub fn is_empty_root(repo: &git2::Repository, tree: &git2::Tree) -> bool {
    if tree.id() == empty_tree_id() {
        return true;
    }

    let mut all_empty = true;

    for e in tree.iter() {
        if let Ok(Ok(t)) = e.to_object(&repo).map(|x| x.into_tree()) {
            all_empty = all_empty && is_empty_root(&repo, &t);
        } else {
            return false;
        }
    }
    return all_empty;
}

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
    let mut result = empty_tree(&repo);

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

            if s != empty_tree_id() {
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

pub fn substract_tree<'a>(
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
    rs_tracing::trace_scoped!("substract_tree X", "root": root);

    let tree = repo.find_tree(input)?;
    let mut result = empty_tree(&repo);

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
                substract_tree(
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

            if s != empty_tree_id() {
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

pub fn substract_fast(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    if input1 == input2 {
        return Ok(empty_tree_id());
    }
    if input1 == empty_tree_id() {
        return Ok(empty_tree_id());
    }

    if let (Ok(tree1), Ok(tree2)) =
        (repo.find_tree(input1), repo.find_tree(input2))
    {
        if input2 == empty_tree_id() {
            return Ok(input1);
        }
        rs_tracing::trace_scoped!("substract fast");
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
                    substract_fast(repo, e.id(), entry.id())?,
                    &result_tree,
                )?;
            }
        }

        return Ok(result_tree.id());
    }

    return Ok(empty_tree_id());
}

pub fn replace_child<'a>(
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
        } else if oid == empty_tree_id() {
            builder.remove(child).ok();
        } else {
            builder.insert(child, oid, mode).ok();
        }
        builder.write()?
    };
    return Ok(repo.find_tree(full_tree_id)?);
}

fn get_subtree(tree: &git2::Tree, path: &std::path::Path) -> Option<git2::Oid> {
    tree.get_path(path).map(|x| x.id()).ok()
}

pub fn replace_subtree<'a>(
    repo: &'a git2::Repository,
    path: &std::path::Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> super::JoshResult<git2::Tree<'a>> {
    if path.components().count() == 1 {
        return replace_child(&repo, path, oid, full_tree);
    } else {
        let name = std::path::Path::new(
            path.file_name().ok_or(super::josh_error("file_name"))?,
        );
        let path = path.parent().ok_or(super::josh_error("path.parent"))?;

        let st = if let Some(st) = get_subtree(&full_tree, path) {
            repo.find_tree(st).unwrap_or(empty_tree(&repo))
        } else {
            empty_tree(&repo)
        };

        let tree = replace_child(&repo, name, oid, &st)?;

        return replace_subtree(&repo, path, tree.id(), full_tree);
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
    if input1 == empty_tree_id() {
        return Ok(input2);
    }
    if input2 == empty_tree_id() {
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
    trees: Vec<(&super::filters::Filter, git2::Tree<'a>)>,
) -> super::JoshResult<git2::Tree<'a>> {
    rs_tracing::trace_scoped!("compose");
    let mut result = empty_tree(&repo);
    let mut taken = empty_tree(&repo);
    for (f, applied) in trees {
        let taken_applied = super::filters::apply(&repo, &f, taken.clone())?;
        let substracted = repo.find_tree(substract_fast(
            &repo,
            applied.id(),
            taken_applied.id(),
        )?)?;
        taken = super::filters::unapply(&repo, &f, applied, taken.clone())?;
        result =
            repo.find_tree(overlay(&repo, result.id(), substracted.id())?)?;
    }

    Ok(result)
}
