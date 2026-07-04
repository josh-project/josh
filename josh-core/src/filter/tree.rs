use super::*;
use anyhow::anyhow;

pub fn pathstree<'a>(
    root: &str,
    input: git2::Oid,
    transaction: &'a cache::Transaction,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_paths((input, root.to_string())) {
        return Ok(repo.find_tree(cached)?);
    }

    let tree = repo.find_tree(input)?;
    let mut result = empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let path = normalize_path(&Path::new(root).join(name));
            let path_string = path.to_str().ok_or_else(|| anyhow!("no name"))?;
            let file_contents = if name == "workspace.josh" {
                format!(
                    "#{}\n{}",
                    path_string,
                    get_blob(repo, &tree, Path::new(&name))
                )
            } else {
                path_string.to_string()
            };
            result = replace_child(
                repo,
                Path::new(name),
                repo.blob(file_contents.as_bytes())?,
                0o0100644,
                &result,
            )?;
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = pathstree(
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                entry.id(),
                transaction,
            )?
            .id();

            if s != empty_id() {
                result = replace_child(repo, Path::new(name), s, 0o0040000, &result)?;
            }
        }
    }
    transaction.insert_paths((input, root.to_string()), result.id());
    Ok(result)
}

pub fn regex_replace<'a>(
    input: git2::Oid,
    regex: &regex::Regex,
    replacement: &str,
    transaction: &'a cache::Transaction,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();

    let tree = repo.find_tree(input)?;
    let mut result = tree::empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(anyhow!("no name"))?;
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let file_contents = get_blob(repo, &tree, std::path::Path::new(&name));
            let replaced = regex.replacen(&file_contents, 0, replacement);

            result = replace_child(
                repo,
                std::path::Path::new(name),
                repo.blob(replaced.as_bytes())?,
                entry.filemode(),
                &result,
            )?;
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = regex_replace(entry.id(), regex, replacement, transaction)?.id();

            if s != tree::empty_id() {
                result = replace_child(repo, std::path::Path::new(name), s, 0o0040000, &result)?;
            }
        }
    }
    Ok(result)
}

pub fn remove_pred<'a>(
    transaction: &'a cache::Transaction,
    root: &str,
    input: git2::Oid,
    pred: &dyn Fn(&Path, bool) -> bool,
    key: git2::Oid,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_glob((input, key)) {
        return Ok(repo.find_tree(cached)?);
    }
    rs_tracing::trace_scoped!("remove_pred X", "root": root);

    let tree = repo.find_tree(input)?;
    let mut result = empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("INVALID_FILENAME"))?;
        let path = std::path::PathBuf::from(root).join(name);

        if entry.kind() == Some(git2::ObjectType::Blob) && pred(&path, true) {
            result = replace_child(
                repo,
                Path::new(entry.name().ok_or_else(|| anyhow!("no name"))?),
                entry.id(),
                entry.filemode(),
                &result,
            )?;
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = if !root.is_empty() && pred(&path, false) {
                entry.id()
            } else {
                remove_pred(
                    transaction,
                    &format!(
                        "{}{}{}",
                        root,
                        if root.is_empty() { "" } else { "/" },
                        entry.name().ok_or_else(|| anyhow!("no name"))?
                    ),
                    entry.id(),
                    &pred,
                    key,
                )?
                .id()
            };

            if s != empty_id() {
                result = replace_child(
                    repo,
                    Path::new(entry.name().ok_or_else(|| anyhow!("no name"))?),
                    s,
                    0o0040000,
                    &result,
                )?;
            }
        }
    }

    transaction.insert_glob((input, key), result.id());
    Ok(result)
}

pub fn subtract(
    transaction: &cache::Transaction,
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let repo = transaction.repo();
    if input1 == input2 {
        return Ok(empty_id());
    }
    if input1 == empty_id() {
        return Ok(empty_id());
    }

    if let Some(cached) = transaction.get_subtract((input1, input2)) {
        return Ok(cached);
    }

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        if input2 == empty_id() {
            return Ok(input1);
        }
        rs_tracing::trace_scoped!("subtract fast");
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1.get_name(entry.name().ok_or_else(|| anyhow!("no name"))?) {
                result_tree = replace_child(
                    repo,
                    Path::new(entry.name().ok_or_else(|| anyhow!("no name"))?),
                    subtract(transaction, e.id(), entry.id())?,
                    e.filemode(),
                    &result_tree,
                )?;
            }
        }

        transaction.insert_subtract((input1, input2), result_tree.id());

        return Ok(result_tree.id());
    }

    transaction.insert_subtract((input1, input2), empty_id());

    Ok(empty_id())
}

fn replace_child<'a>(
    repo: &'a git2::Repository,
    child: &Path,
    oid: git2::Oid,
    mode: i32,
    full_tree: &git2::Tree,
) -> anyhow::Result<git2::Tree<'a>> {
    let full_tree_id = {
        let mut builder = repo.treebuilder(Some(full_tree))?;
        if oid == git2::Oid::zero() || oid == empty_id() {
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
    path: &Path,
    oid: git2::Oid,
    mode: i32,
) -> anyhow::Result<git2::Tree<'a>> {
    if path.components().count() == 1 {
        replace_child(repo, path, oid, mode, full_tree)
    } else {
        let name = Path::new(path.file_name().ok_or_else(|| anyhow!("file_name"))?);
        let path = path.parent().ok_or_else(|| anyhow!("path.parent"))?;

        let st = if let Ok(st) = full_tree.get_path(path) {
            repo.find_tree(st.id()).unwrap_or(empty(repo))
        } else {
            empty(repo)
        };

        let tree = replace_child(repo, name, oid, mode, &st)?;

        insert(repo, full_tree, path, tree.id(), 0o0040000)
    }
}

pub fn diff_paths(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
    root: &str,
) -> anyhow::Result<Vec<(String, i32)>> {
    rs_tracing::trace_scoped!("diff_paths");
    if input1 == input2 {
        return Ok(vec![]);
    }

    if let (Ok(_), Ok(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), 0)]);
    }

    if let (Ok(_), Err(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), -1)]);
    }

    if let (Err(_), Ok(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), 1)]);
    }

    let mut r = vec![];

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            if let Some(e) = tree1.get_name(entry.name().ok_or_else(|| anyhow!("no name"))?) {
                r.append(&mut diff_paths(
                    repo,
                    e.id(),
                    entry.id(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            } else {
                r.append(&mut diff_paths(
                    repo,
                    git2::Oid::zero(),
                    entry.id(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        for entry in tree1.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            if tree2
                .get_name(entry.name().ok_or_else(|| anyhow!("no name"))?)
                .is_none()
            {
                r.append(&mut diff_paths(
                    repo,
                    entry.id(),
                    git2::Oid::zero(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        return Ok(r);
    }

    if let Ok(tree2) = repo.find_tree(input2) {
        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            r.append(&mut diff_paths(
                repo,
                git2::Oid::zero(),
                entry.id(),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    if let Ok(tree1) = repo.find_tree(input2) {
        for entry in tree1.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            r.append(&mut diff_paths(
                repo,
                entry.id(),
                git2::Oid::zero(),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    Ok(r)
}

pub fn overlay(
    transaction: &cache::Transaction,
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    if let Some(cached) = transaction.get_overlay((input1, input2)) {
        return Ok(cached);
    }
    let repo = transaction.repo();
    if input1 == input2 {
        return Ok(input1);
    }
    if input1 == empty_id() {
        return Ok(input2);
    }
    if input2 == empty_id() {
        return Ok(input1);
    }

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        rs_tracing::trace_begin!( "overlay",
            "overlay_a": format!("{}", input1),
            "overlay_b": format!("{}", input2),
            "overlay_ab": format!("{} - {}", input1, input2));
        let mut builder = repo.treebuilder(Some(&tree1))?;

        let mut i = 0;
        for entry in tree2.iter() {
            i += 1;
            let (id, mode) =
                if let Some(e) = tree1.get_name(entry.name().ok_or_else(|| anyhow!("no name"))?) {
                    (overlay(transaction, e.id(), entry.id())?, e.filemode())
                } else {
                    (entry.id(), entry.filemode())
                };

            builder.insert(
                Path::new(entry.name().ok_or_else(|| anyhow!("no name"))?),
                id,
                mode,
            )?;
        }

        let rid = builder.write()?;
        rs_tracing::trace_end!( "overlay", "count":i);

        transaction.insert_overlay((input1, input2), rid);
        return Ok(rid);
    }

    Ok(input1)
}

pub fn pathline(b: &str) -> anyhow::Result<String> {
    match b
        .split('\n')
        .next()
        .map(|line| line.trim_start_matches('#'))
    {
        Some(line) if !line.is_empty() => Ok(line.to_string()),
        Some(_) | None => Err(anyhow!("pathline")),
    }
}

pub fn invert_paths<'a>(
    transaction: &'a cache::Transaction,
    root: &str,
    tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_invert((tree.id(), root.to_string())) {
        return Ok(repo.find_tree(cached)?);
    }

    let mut result = empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;

        if entry.kind() == Some(git2::ObjectType::Blob) {
            let mpath = normalize_path(&Path::new(root).join(name))
                .to_string_lossy()
                .to_string();
            let b = get_blob(repo, &tree, Path::new(name));
            let opath = pathline(&b)?;

            result = insert(
                repo,
                &result,
                Path::new(&opath),
                repo.blob(mpath.as_bytes())?,
                0o0100644,
            )
            .unwrap();
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = invert_paths(
                transaction,
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                repo.find_tree(entry.id())?,
            )?;
            result = repo.find_tree(overlay(transaction, result.id(), s.id())?)?;
        }
    }

    transaction.insert_invert((tree.id(), root.to_string()), result.id());

    Ok(result)
}

pub fn original_path(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: git2::Tree,
    path: &Path,
) -> anyhow::Result<String> {
    let paths_tree = apply(
        transaction,
        to_filter(Op::Paths).chain(filter),
        Rewrite::from_tree(tree),
    )?;
    let b = get_blob(transaction.repo(), paths_tree.tree(), path);
    pathline(&b)
}

pub fn repopulated_tree(
    transaction: &cache::Transaction,
    filter: Filter,
    full_tree: git2::Tree,
    partial_tree: git2::Tree,
) -> anyhow::Result<git2::Oid> {
    let paths_tree = apply(
        transaction,
        to_filter(Op::Paths).chain(filter),
        Rewrite::from_tree(full_tree),
    )?;

    let ipaths = invert_paths(transaction, "", paths_tree.into_tree())?;
    populate(transaction, ipaths.id(), partial_tree.id())
}

pub fn populate(
    transaction: &cache::Transaction,
    paths: git2::Oid,
    content: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    rs_tracing::trace_scoped!("repopulate");

    if let Some(cached) = transaction.get_populate((paths, content)) {
        return Ok(cached);
    }

    let repo = transaction.repo();

    let mut result_tree = empty_id();
    if let (Ok(paths), Ok(content)) = (repo.find_blob(paths), repo.find_blob(content)) {
        let ipath = pathline(std::str::from_utf8(paths.content())?)?;
        result_tree = insert(
            repo,
            &repo.find_tree(result_tree)?,
            Path::new(&ipath),
            content.id(),
            0o0100644,
        )?
        .id();
    } else if let (Ok(paths), Ok(content)) = (repo.find_tree(paths), repo.find_tree(content)) {
        for entry in content.iter() {
            if let Some(e) = paths.get_name(entry.name().ok_or_else(|| anyhow!("no name"))?) {
                result_tree = overlay(
                    transaction,
                    result_tree,
                    populate(transaction, e.id(), entry.id())?,
                )?;
            }
        }
    }

    transaction.insert_populate((paths, content), result_tree);

    Ok(result_tree)
}

pub fn compose_fast(
    transaction: &cache::Transaction,
    trees: Vec<git2::Oid>,
) -> anyhow::Result<git2::Tree<'_>> {
    rs_tracing::trace_scoped!("compose_fast");
    let repo = transaction.repo();
    let mut result = empty_id();
    for tree in trees {
        result = overlay(transaction, tree, result)?;
    }

    Ok(repo.find_tree(result)?)
}

pub fn compose<'a>(
    transaction: &'a cache::Transaction,
    trees: Vec<(&Filter, git2::Tree<'a>)>,
) -> anyhow::Result<git2::Tree<'a>> {
    rs_tracing::trace_scoped!("compose");
    let repo = transaction.repo();
    let mut result = empty(repo);
    let mut taken = empty(repo);
    for (f, applied) in trees {
        let tid = taken.id();
        // If a filter creates a tree entry that does not exist in the input (Like TreeId and Blob),
        // the "output uniqueness handling" will cause it's output entry to be removed from the
        // tree during compose.
        // Note that f is only used for uniqueness calculation in this function so normalizing
        // it using double invert is ok and and does not affect the output of the filter itself,
        // since the original filter was already applied by the caller and passed via the "trees"
        // parameter.
        let f = invert(invert(*f)?)?;
        let taken_applied = if let Some(cached) = transaction.get_apply(f, tid) {
            cached
        } else {
            apply(transaction, f, Rewrite::from_tree(taken.clone()))?
                .tree()
                .id()
        };
        transaction.insert_apply(f, tid, taken_applied);

        let subtracted = repo.find_tree(subtract(transaction, applied.id(), taken_applied)?)?;

        let aid = applied.id();
        let unapplied = if let Some(cached) = transaction.get_unapply(f, aid) {
            cached
        } else {
            apply(transaction, invert(f)?, Rewrite::from_tree(applied))?
                .tree()
                .id()
        };
        transaction.insert_unapply(f, aid, unapplied);
        taken = repo.find_tree(overlay(transaction, taken.id(), unapplied)?)?;
        result = repo.find_tree(overlay(transaction, subtracted.id(), result.id())?)?;
    }

    Ok(result)
}

pub fn get_blob(repo: &git2::Repository, tree: &git2::Tree, path: &Path) -> String {
    let entry_oid = ok_or!(tree.get_path(path).map(|x| x.id()), {
        return "".to_owned();
    });

    let blob = ok_or!(repo.find_blob(entry_oid), {
        return "".to_owned();
    });

    if blob.is_binary() {
        return "".to_owned();
    }

    let content = ok_or!(std::str::from_utf8(blob.content()), {
        return "".to_owned();
    });

    content.to_owned()
}

pub fn empty_id() -> git2::Oid {
    git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap()
}

pub fn empty(repo: &git2::Repository) -> git2::Tree<'_> {
    repo.find_tree(empty_id()).unwrap()
}
