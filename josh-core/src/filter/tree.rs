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
    let mut builder = repo.treebuilder(None)?;

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
            builder
                .insert(name, repo.blob(file_contents.as_bytes())?, 0o0100644)
                .ok();
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = pathstree(
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                entry.id(),
                transaction,
            )?
            .id();

            if s != empty_id() {
                builder.insert(name, s, 0o0040000).ok();
            }
        }
    }
    let result = repo.find_tree(builder.write()?)?;
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
    let mut builder = repo.treebuilder(None)?;

    for entry in tree.iter() {
        let name = entry.name().ok_or(anyhow!("no name"))?;
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let file_contents = get_blob(repo, &tree, std::path::Path::new(&name));
            let replaced = regex.replacen(&file_contents, 0, replacement);

            builder
                .insert(name, repo.blob(replaced.as_bytes())?, entry.filemode())
                .ok();
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = regex_replace(entry.id(), regex, replacement, transaction)?.id();

            if s != tree::empty_id() {
                builder.insert(name, s, 0o0040000).ok();
            }
        }
    }
    Ok(repo.find_tree(builder.write()?)?)
}

/// Compare two tree entry names in canonical git tree order: byte-wise, with tree entries
/// sorted as if their name had a trailing '/'.
fn git_tree_entry_cmp(a: &[u8], a_is_tree: bool, b: &[u8], b_is_tree: bool) -> std::cmp::Ordering {
    let len = a.len().min(b.len());
    match a[..len].cmp(&b[..len]) {
        std::cmp::Ordering::Equal => {}
        ord => return ord,
    }
    let ca = a
        .get(len)
        .copied()
        .unwrap_or(if a_is_tree { b'/' } else { 0 });
    let cb = b
        .get(len)
        .copied()
        .unwrap_or(if b_is_tree { b'/' } else { 0 });
    ca.cmp(&cb)
}

/// Rebuild `input` keeping only blob entries accepted by `pred`. `path` is a reusable buffer
/// holding the slash-separated path of the tree currently being visited; it is restored to its
/// incoming length before returning. Gitlink (submodule) entries are always dropped.
pub fn remove_pred(
    transaction: &cache::Transaction,
    path: &mut String,
    input: git2::Oid,
    pred: &dyn Fn(&str, bool) -> bool,
    key: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    if let Some(cached) = transaction.get_glob((input, key)) {
        return Ok(cached);
    }

    let repo = transaction.repo();
    let tree = repo.find_tree(input)?;
    let mut builder = repo.treebuilder(None)?;
    let empty = empty_id();
    let mut changed = false;
    // Previous entry, used to verify the input tree is in canonical git order with no duplicate
    // names. Non-canonical trees (fsck-invalid, but transportable with default git settings) were
    // normalized by the old unconditional `builder.write()`, so they must not take the unchanged
    // fast path.
    let mut prev: Option<(git2::TreeEntry, bool)> = None;

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("INVALID_FILENAME"))?;
        let base = path.len();
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(name);

        let is_tree = entry.kind() == Some(git2::ObjectType::Tree);
        if let Some((prev_entry, prev_is_tree)) = &prev {
            let prev_name = prev_entry.name_bytes();
            if prev_name == name.as_bytes()
                || git_tree_entry_cmp(prev_name, *prev_is_tree, name.as_bytes(), is_tree)
                    != std::cmp::Ordering::Less
            {
                changed = true;
            }
        }

        match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                if pred(path, true) {
                    // `filemode()` is the libgit2-normalized mode: if it differs from the raw
                    // on-disk mode (e.g. legacy 100664 blobs), the rebuilt tree differs from the
                    // input. Failed inserts (names the treebuilder rejects, like ".git") were
                    // silently dropped by the old code, so they also count as changed.
                    if entry.filemode_raw() != entry.filemode() {
                        changed = true;
                    }
                    if builder.insert(name, entry.id(), entry.filemode()).is_err() {
                        changed = true;
                    }
                } else {
                    changed = true;
                }
            }
            Some(git2::ObjectType::Tree) => {
                let s = remove_pred(transaction, path, entry.id(), pred, key)?;
                if s != entry.id() || s == empty || entry.filemode_raw() != 0o0040000 {
                    changed = true;
                }
                if s != empty && builder.insert(name, s, 0o0040000).is_err() {
                    changed = true;
                }
            }
            // Gitlinks (and any other kinds) are dropped, so the rebuilt tree differs from the
            // input and must not take the unchanged fast path below.
            _ => {
                changed = true;
            }
        }
        path.truncate(base);
        prev = Some((entry, is_tree));
    }

    // If nothing was dropped or rewritten, the builder holds exactly the input's entries; since
    // git trees are content-addressed, writing it out would reproduce `input` bit-identically.
    let result = if changed {
        builder.write()?
    } else {
        debug_assert_eq!(builder.write()?, input);
        input
    };
    transaction.insert_glob((input, key), result);
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
        // Start from `tree1` and drop or replace each path that also appears in `tree2`.
        let mut builder = repo.treebuilder(Some(&tree1))?;

        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            if let Some(e) = tree1.get_name(name) {
                let sub = subtract(transaction, e.id(), entry.id())?;
                if sub == empty_id() || sub == git2::Oid::zero() {
                    builder.remove(name).ok();
                } else {
                    builder.insert(name, sub, e.filemode()).ok();
                }
            }
        }

        let result = builder.write()?;

        transaction.insert_subtract((input1, input2), result);

        return Ok(result);
    }

    transaction.insert_subtract((input1, input2), empty_id());

    Ok(empty_id())
}

/// Intersect two trees by path: keep every entry of `input1` whose path also exists in `input2`,
/// carrying `input1`'s content and mode. This is the exact complement of [`subtract`] over `input1`
/// -- `subtract` drops the shared paths, `intersect` keeps them -- so
/// `intersect(a, b) == subtract(a, subtract(a, b))`. Computing it directly (rather than via that
/// double subtract) matters for performance: the double subtract's outer step iterates `a`'s
/// complement, which is nearly all of `a`, whereas this iterates only `input2`. Selecting a small
/// set of paths out of a large tree therefore costs O(input2) instead of O(input1).
pub fn intersect(
    transaction: &cache::Transaction,
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let repo = transaction.repo();
    // Identical (sub)trees intersect to themselves; an empty side leaves nothing to keep.
    if input1 == input2 {
        return Ok(input1);
    }
    if input1 == empty_id() || input2 == empty_id() {
        return Ok(empty_id());
    }

    if let Some(cached) = transaction.get_intersect((input1, input2)) {
        return Ok(cached);
    }

    let result = if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        // Iterate the selector (`input2`), keeping each of its paths that also exists in `tree1`
        // with `tree1`'s content; cost tracks the size of the selected set.
        let mut builder = repo.treebuilder(None)?;
        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            if let Some(e1) = tree1.get_name(name) {
                let child = intersect(transaction, e1.id(), entry.id())?;
                if child != empty_id() && child != git2::Oid::zero() {
                    builder.insert(name, child, e1.filemode()).ok();
                }
            }
        }
        builder.write()?
    } else {
        // At least one side is a blob at this already-name-matched path, so the path exists in both;
        // keep `input1`'s content, matching the path-based semantics of the tree case.
        input1
    };

    transaction.insert_intersect((input1, input2), result);

    Ok(result)
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

    if let Ok(tree1) = repo.find_tree(input1) {
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
        let mut builder = repo.treebuilder(Some(&tree1))?;

        for entry in tree2.iter() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(repo: &git2::Repository, paths: &[&str]) -> git2::Oid {
        let mut b = git2::build::TreeUpdateBuilder::new();
        for p in paths {
            let oid = repo.blob(p.as_bytes()).unwrap();
            b.upsert(*p, oid, git2::FileMode::Blob);
        }
        let base = repo.treebuilder(None).unwrap().write().unwrap();
        b.create_updated(repo, &repo.find_tree(base).unwrap())
            .unwrap()
    }

    fn open_transaction(td: &tempfile::TempDir) -> cache::Transaction {
        cache::sled_load(td.path()).unwrap();
        let ctx = cache::TransactionContext::new(td.path(), cache::CacheStack::default().into());
        ctx.open().unwrap()
    }

    // A gitlink (submodule) entry must be dropped from the rebuilt tree -- and must therefore
    // defeat the "unchanged input" fast path -- while a symlink blob accepted by the predicate
    // keeps its 0o120000 filemode.
    #[test]
    fn remove_pred_drops_gitlink_and_preserves_symlink_mode() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let blob = repo.blob(b"content").unwrap();
        let link = repo.blob(b"target").unwrap();
        // Gitlinks reference commits in other repositories; libgit2 does not require the oid
        // to exist locally.
        let sub = git2::Oid::from_str("0123456789012345678901234567890123456789").unwrap();

        let mut b = repo.treebuilder(None).unwrap();
        b.insert("keep.rs", blob, 0o100644).unwrap();
        b.insert("link.rs", link, 0o120000).unwrap();
        b.insert("sub", sub, 0o160000).unwrap();
        let input = b.write().unwrap();

        let t = open_transaction(&td);
        let key = git2::Oid::from_str("1111111111111111111111111111111111111111").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();

        assert_ne!(out, input, "dropping the gitlink must produce a new tree");
        let out_tree = t.repo().find_tree(out).unwrap();
        assert!(
            out_tree.get_name("sub").is_none(),
            "gitlink must be dropped"
        );
        assert!(out_tree.get_name("keep.rs").is_some());
        let link_entry = out_tree.get_name("link.rs").expect("symlink kept");
        assert_eq!(link_entry.filemode(), 0o120000);
        assert_eq!(link_entry.id(), link);
    }

    // The predicate must see full slash-separated paths at every depth (truncate discipline of
    // the shared path buffer), and a keep-everything predicate must return the input oid via the
    // unchanged fast path.
    #[test]
    fn remove_pred_passes_full_paths_and_reuses_unchanged_input() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let paths = ["a/b/drop.txt", "a/b/keep.rs", "a/keep.rs", "top.rs"];
        let input = make_tree(&repo, &paths);

        let t = open_transaction(&td);
        let key = git2::Oid::from_str("2222222222222222222222222222222222222222").unwrap();

        let seen = std::cell::RefCell::new(Vec::new());
        let pred = |path: &str, isblob: bool| {
            assert!(isblob, "predicate must only be called for blobs");
            seen.borrow_mut().push(path.to_string());
            path.ends_with(".rs")
        };
        let out = remove_pred(&t, &mut String::new(), input, &pred, key).unwrap();

        let mut seen = seen.into_inner();
        seen.sort();
        assert_eq!(seen, paths);

        let out_tree = t.repo().find_tree(out).unwrap();
        for kept in ["a/b/keep.rs", "a/keep.rs", "top.rs"] {
            assert!(out_tree.get_path(Path::new(kept)).is_ok(), "{kept} kept");
        }
        assert!(out_tree.get_path(Path::new("a/b/drop.txt")).is_err());

        let key2 = git2::Oid::from_str("3333333333333333333333333333333333333333").unwrap();
        let out2 = remove_pred(&t, &mut String::new(), input, &|_, _| true, key2).unwrap();
        assert_eq!(out2, input, "keep-everything must return the input oid");
    }

    // Write a raw (unvalidated) tree object straight into the odb. This can express fsck-invalid
    // trees -- legacy filemodes, unsorted or duplicate entries, forbidden names -- that git can
    // still transport with default settings and that therefore reach remove_pred in production.
    fn write_raw_tree(repo: &git2::Repository, entries: &[(&str, &str, git2::Oid)]) -> git2::Oid {
        let mut data = Vec::new();
        for (mode, name, oid) in entries {
            data.extend_from_slice(mode.as_bytes());
            data.push(b' ');
            data.extend_from_slice(name.as_bytes());
            data.push(0);
            data.extend_from_slice(oid.as_bytes());
        }
        repo.odb()
            .unwrap()
            .write(git2::ObjectType::Tree, &data)
            .unwrap()
    }

    // A legacy blob mode like 100664 is normalized by the treebuilder, so a keep-everything
    // predicate must NOT return the raw input oid: it must return the normalized rewrite,
    // exactly like the old unconditional builder.write() did.
    #[test]
    fn remove_pred_normalizes_legacy_filemode() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let blob = repo.blob(b"content").unwrap();
        let input = write_raw_tree(&repo, &[("100664", "file.rs", blob)]);
        assert_ne!(
            repo.find_tree(input)
                .unwrap()
                .get_name("file.rs")
                .unwrap()
                .filemode_raw(),
            0o100644,
            "input must carry the raw legacy mode"
        );

        let mut b = repo.treebuilder(None).unwrap();
        b.insert("file.rs", blob, 0o100644).unwrap();
        let expected = b.write().unwrap();
        assert_ne!(expected, input);

        let t = open_transaction(&td);
        let key = git2::Oid::from_str("4444444444444444444444444444444444444444").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_eq!(
            out, expected,
            "legacy mode must be normalized, not passed through"
        );
    }

    // Entries the treebuilder rejects (".git") were silently dropped by the old code, and
    // non-canonical entry order was normalized by the old unconditional builder.write(). Both
    // must still happen instead of returning the fsck-invalid input via the fast path.
    #[test]
    fn remove_pred_normalizes_invalid_names_and_entry_order() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();
        let blob = repo.blob(b"content").unwrap();
        let t = open_transaction(&td);

        // ".git" is rejected by the treebuilder: the old code dropped it via .ok().
        let input = write_raw_tree(
            &repo,
            &[("100644", ".git", blob), ("100644", "keep.rs", blob)],
        );
        let key = git2::Oid::from_str("5555555555555555555555555555555555555555").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_ne!(out, input);
        let out_tree = t.repo().find_tree(out).unwrap();
        assert!(out_tree.get_name(".git").is_none(), ".git must be dropped");
        assert!(out_tree.get_name("keep.rs").is_some());

        // Unsorted input: the old code always rewrote it in canonical order.
        let input = write_raw_tree(&repo, &[("100644", "b.rs", blob), ("100644", "a.rs", blob)]);
        let mut b = repo.treebuilder(None).unwrap();
        b.insert("a.rs", blob, 0o100644).unwrap();
        b.insert("b.rs", blob, 0o100644).unwrap();
        let expected = b.write().unwrap();
        assert_ne!(expected, input);
        let key = git2::Oid::from_str("6666666666666666666666666666666666666666").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_eq!(
            out, expected,
            "unsorted input must be rewritten in canonical order"
        );

        // Duplicate names: last one wins in the treebuilder, exactly as the old code behaved.
        let blob2 = repo.blob(b"other").unwrap();
        let input = write_raw_tree(
            &repo,
            &[("100644", "a.rs", blob), ("100644", "a.rs", blob2)],
        );
        let mut b = repo.treebuilder(None).unwrap();
        b.insert("a.rs", blob2, 0o100644).unwrap();
        let expected = b.write().unwrap();
        let key = git2::Oid::from_str("7777777777777777777777777777777777777777").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_eq!(out, expected, "duplicate entries must be deduplicated");
    }

    // Removing a whole subdirectory must report every file under it as removed. This exercises
    // the "input1 is a tree, input2 is gone" branch of diff_paths, which is only reachable via
    // the recursion for entries present in tree1 but absent from tree2.
    #[test]
    fn diff_paths_reports_removed_subtree() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let tree1 = make_tree(&repo, &["dir/file1", "dir/file2", "kept"]);
        let tree2 = make_tree(&repo, &["kept"]);

        let removed = diff_paths(&repo, tree1, tree2, "").unwrap();
        assert_eq!(
            removed,
            vec![("dir/file1".to_string(), -1), ("dir/file2".to_string(), -1)]
        );

        let added = diff_paths(&repo, tree2, tree1, "").unwrap();
        assert_eq!(
            added,
            vec![("dir/file1".to_string(), 1), ("dir/file2".to_string(), 1)]
        );
    }
}
