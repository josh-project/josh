use super::*;

pub fn pathstree<'a>(
    root: &str,
    input: git2::Oid,
    transaction: &'a cache::Transaction,
) -> super::JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_paths((input, root.to_string())) {
        return Ok(repo.find_tree(cached)?);
    }

    let tree = repo.find_tree(input)?;
    let mut result = tree::empty(&repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(super::josh_error("no name"))?;
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let file_contents;
            let path = normalize_path(&std::path::Path::new(root).join(name));
            let path_string = path
                .to_str()
                .ok_or(super::josh_error("no name"))?;
            if name == "workspace.josh" {
                file_contents = format!(
                    "#{}\n{}",
                    path_string,
                    get_blob(repo, &tree, &std::path::Path::new(&name))
                )
                .to_string();
            } else {
                file_contents = path_string.to_string();
            }
            result = replace_child(
                &repo,
                &std::path::Path::new(name),
                repo.blob(file_contents.as_bytes())?,
                0o0100644,
                &result,
            )?;
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = pathstree(
                &format!("{}{}{}", root, if root == "" { "" } else { "/" }, name),
                entry.id(),
                transaction,
            )?
            .id();

            if s != tree::empty_id() {
                result = replace_child(&repo, &std::path::Path::new(name), s, 0o0040000, &result)?;
            }
        }
    }
    transaction.insert_paths((input, root.to_string()), result.id());
    return Ok(result);
}

pub fn remove_pred<'a>(
    transaction: &'a cache::Transaction,
    root: &str,
    input: git2::Oid,
    pred: &dyn Fn(&std::path::Path, bool) -> bool,
    key: git2::Oid,
) -> super::JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_glob((input, key)) {
        return Ok(repo.find_tree(cached)?);
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
                    &std::path::Path::new(entry.name().ok_or(super::josh_error("no name"))?),
                    entry.id(),
                    entry.filemode(),
                    &result,
                )?;
            }
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = if (root != "") && pred(&path, false) {
                entry.id()
            } else {
                remove_pred(
                    transaction,
                    &format!(
                        "{}{}{}",
                        root,
                        if root == "" { "" } else { "/" },
                        entry.name().ok_or(super::josh_error("no name"))?
                    ),
                    entry.id(),
                    &pred,
                    key,
                )?
                .id()
            };

            if s != tree::empty_id() {
                result = replace_child(
                    &repo,
                    &std::path::Path::new(entry.name().ok_or(super::josh_error("no name"))?),
                    s,
                    0o0040000,
                    &result,
                )?;
            }
        }
    }

    transaction.insert_glob((input, key), result.id());
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

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        if input2 == tree::empty_id() {
            return Ok(input1);
        }
        rs_tracing::trace_scoped!("subtract fast");
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1.get_name(entry.name().ok_or(super::josh_error("no name"))?) {
                result_tree = replace_child(
                    &repo,
                    &std::path::Path::new(entry.name().ok_or(super::josh_error("no name"))?),
                    subtract(repo, e.id(), entry.id())?,
                    e.filemode(),
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
    mode: i32,
    full_tree: &git2::Tree,
) -> super::JoshResult<git2::Tree<'a>> {
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
    mode: i32,
) -> super::JoshResult<git2::Tree<'a>> {
    if path.components().count() == 1 {
        return replace_child(&repo, path, oid, mode, full_tree);
    } else {
        let name = std::path::Path::new(path.file_name().ok_or(super::josh_error("file_name"))?);
        let path = path.parent().ok_or(super::josh_error("path.parent"))?;

        let st = if let Ok(st) = full_tree.get_path(path) {
            repo.find_tree(st.id()).unwrap_or(tree::empty(&repo))
        } else {
            tree::empty(&repo)
        };

        let tree = replace_child(&repo, name, oid, mode, &st)?;

        return insert(&repo, full_tree, path, tree.id(), 0o0040000);
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

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1.get_name(entry.name().ok_or(super::josh_error("no name"))?) {
                result_tree = replace_child(
                    &repo,
                    &std::path::Path::new(entry.name().ok_or(super::josh_error("no name"))?),
                    overlay(repo, entry.id(), e.id())?,
                    e.filemode(),
                    &result_tree,
                )?;
            } else {
                result_tree = replace_child(
                    &repo,
                    &std::path::Path::new(entry.name().ok_or(super::josh_error("no name"))?),
                    entry.id(),
                    entry.filemode(),
                    &result_tree,
                )?;
            }
        }

        return Ok(result_tree.id());
    }

    return Ok(input1);
}

pub fn pathline(b: &str) -> JoshResult<String> {
    for line in b.split("\n") {
        let l = line.trim_start_matches("#");
        if l == "" {
            break;
        }
        return Ok(l.to_string());
    }
    return Err(josh_error("pathline"));
}

pub fn invert_paths<'a>(
    transaction: &'a cache::Transaction,
    root: &str,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_invert((tree.id(), root.to_string())) {
        return Ok(repo.find_tree(cached)?);
    }

    let mut result = tree::empty(&repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(super::josh_error("no name"))?;

        if entry.kind() == Some(git2::ObjectType::Blob) {
            let mpath = normalize_path(&std::path::Path::new(root).join(name))
                .to_string_lossy()
                .to_string();
            let b = tree::get_blob(&repo, &tree, &std::path::Path::new(name));
            let opath = pathline(&b)?;

            result = insert(
                &repo,
                &result,
                &std::path::Path::new(&opath),
                repo.blob(mpath.as_bytes())?,
                0o0100644,
            )
            .unwrap();
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = invert_paths(
                &transaction,
                &format!("{}{}{}", root, if root == "" { "" } else { "/" }, name),
                repo.find_tree(entry.id())?,
            )?;
            result = repo.find_tree(overlay(&repo, result.id(), s.id())?)?;
        }
    }

    transaction.insert_invert((tree.id(), root.to_string()), result.id());

    return Ok(result);
}

pub fn original_path(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: git2::Tree,
    path: &std::path::Path,
) -> JoshResult<String> {
    let paths_tree = apply(transaction, chain(to_filter(Op::Paths), filter), tree)?;
    let b = tree::get_blob(transaction.repo(), &paths_tree, path);
    return pathline(&b);
}

pub fn repopulated_tree(
    transaction: &cache::Transaction,
    filter: Filter,
    full_tree: git2::Tree,
    partial_tree: git2::Tree,
) -> JoshResult<git2::Oid> {
    let paths_tree = apply(transaction, chain(to_filter(Op::Paths), filter), full_tree)?;

    let ipaths = invert_paths(transaction, "", paths_tree)?;
    populate(transaction, ipaths.id(), partial_tree.id())
}

fn populate(
    transaction: &cache::Transaction,
    paths: git2::Oid,
    content: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("repopulate");

    if let Some(cached) = transaction.get_populate((paths, content)) {
        return Ok(cached);
    }

    let repo = transaction.repo();

    let mut result_tree = empty_id();
    if let (Ok(paths), Ok(content)) = (repo.find_blob(paths), repo.find_blob(content)) {
        let ipath = pathline(&std::str::from_utf8(paths.content())?)?;
        result_tree = insert(
            &repo,
            &repo.find_tree(result_tree)?,
            &std::path::Path::new(&ipath),
            content.id(),
            0o0100644,
        )?
        .id();
    } else if let (Ok(paths), Ok(content)) = (repo.find_tree(paths), repo.find_tree(content)) {
        for entry in content.iter() {
            if let Some(e) = paths.get_name(entry.name().ok_or(super::josh_error("no name"))?) {
                result_tree = overlay(
                    &repo,
                    result_tree,
                    populate(transaction, e.id(), entry.id())?,
                )?;
            }
        }
    }

    transaction.insert_populate((paths, content), result_tree);

    return Ok(result_tree);
}

pub fn compose<'a>(
    transaction: &'a cache::Transaction,
    trees: Vec<(&super::filter::Filter, git2::Tree<'a>)>,
) -> super::JoshResult<git2::Tree<'a>> {
    rs_tracing::trace_scoped!("compose");
    let repo = transaction.repo();
    let mut result = tree::empty(&repo);
    let mut taken = tree::empty(&repo);
    for (f, applied) in trees {
        let tid = taken.id();
        let taken_applied = if let Some(cached) = transaction.get_apply(*f, tid) {
            cached
        } else {
            filter::apply(transaction, *f, taken.clone())?.id()
        };
        transaction.insert_apply(*f, tid, taken_applied);

        let subtracted = repo.find_tree(subtract(&repo, applied.id(), taken_applied)?)?;

        let aid = applied.id();
        let unapplied = if let Some(cached) = transaction.get_unapply(*f, aid) {
            cached
        } else {
            filter::unapply(transaction, *f, applied, empty(&repo))?.id()
        };
        transaction.insert_unapply(*f, aid, unapplied);
        taken = repo.find_tree(overlay(&repo, taken.id(), unapplied)?)?;
        result = repo.find_tree(overlay(&repo, result.id(), subtracted.id())?)?;
    }

    Ok(result)
}

pub fn get_blob(repo: &git2::Repository, tree: &git2::Tree, path: &Path) -> String {
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
    return git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap();
}

pub fn empty(repo: &git2::Repository) -> git2::Tree {
    repo.find_tree(empty_id()).unwrap()
}
