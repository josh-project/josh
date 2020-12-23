use git2;
use tracing;

use super::empty_tree;
use super::empty_tree_id;
use super::filter_cache;
use super::filters;
use super::UnapplyFilter;
use std::collections::HashSet;

fn all_equal(a: git2::Parents, b: &[&git2::Commit]) -> bool {
    let a: Vec<_> = a.collect();
    if a.len() != b.len() {
        return false;
    }

    for (x, y) in b.iter().zip(a.iter()) {
        if x.id() != y.id() {
            return false;
        }
    }
    return true;
}

// takes everything from base except it's tree and replaces it with the tree
// given
pub fn rewrite(
    repo: &git2::Repository,
    base: &git2::Commit,
    parents: &[&git2::Commit],
    tree: &git2::Tree,
) -> super::JoshResult<git2::Oid> {
    if base.tree()?.id() == tree.id() && all_equal(base.parents(), parents) {
        // Looks like an optimization, but in fact serves to not change the commit in case
        // it was signed.
        return Ok(base.id());
    }

    return Ok(repo.commit(
        None,
        &base.author(),
        &base.committer(),
        &base.message_raw().unwrap_or("no message"),
        tree,
        parents,
    )?);
}

fn find_original(
    repo: &git2::Repository,
    bm: &mut std::collections::HashMap<git2::Oid, git2::Oid>,
    filter: &filters::Filter,
    contained_in: git2::Oid,
    filtered: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    if contained_in == git2::Oid::zero() {
        return Ok(git2::Oid::zero());
    }
    if let Some(original) = bm.get(&filtered) {
        return Ok(*original);
    }
    let oid = super::history::walk(&repo, &filter, contained_in)?;
    if oid != git2::Oid::zero() {
        bm.insert(contained_in, oid);
    }
    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(contained_in)?;

    for original in walk {
        let original = original?;
        if filtered == super::history::walk(&repo, &filter, original)? {
            bm.insert(filtered, original);
            return Ok(original);
        }
    }

    return Ok(git2::Oid::zero());
}

#[tracing::instrument(skip(repo))]
pub fn unapply_filter(
    repo: &git2::Repository,
    filterobj: &filters::Filter,
    unfiltered_old: git2::Oid,
    old: git2::Oid,
    new: git2::Oid,
) -> super::JoshResult<UnapplyFilter> {
    let walk = {
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(new)?;
        if let Ok(_) = walk.hide(old) {
            tracing::trace!("walk: hidden {}", old);
        } else {
            tracing::warn!("walk: can't hide");
        }
        walk
    };

    let mut bm = std::collections::HashMap::new();
    let mut ret =
        find_original(&repo, &mut bm, filterobj, unfiltered_old, new)?;
    for rev in walk {
        let rev = rev?;

        let s = tracing::span!(tracing::Level::TRACE, "walk commit", ?rev);
        let _e = s.enter();

        let module_commit = repo.find_commit(rev)?;

        if bm.contains_key(&module_commit.id()) {
            continue;
        }

        let filtered_parent_ids: Vec<_> = module_commit.parent_ids().collect();

        let original_parents: std::result::Result<Vec<_>, _> =
            filtered_parent_ids
                .iter()
                .map(|x| -> super::JoshResult<_> {
                    find_original(&repo, &mut bm, filterobj, unfiltered_old, *x)
                })
                .filter(|x| {
                    if let Ok(i) = x {
                        *i != git2::Oid::zero()
                    } else {
                        true
                    }
                })
                .map(|x| -> super::JoshResult<_> { Ok(repo.find_commit(x?)?) })
                .collect();

        tracing::info!(
            "parents: {:?} -> {:?}",
            original_parents,
            filtered_parent_ids
        );

        let original_parents = original_parents?;

        let original_parents_refs: Vec<&git2::Commit> =
            original_parents.iter().collect();

        let tree = module_commit.tree()?;

        let commit_message =
            module_commit.summary().unwrap_or("NO COMMIT MESSAGE");

        let new_trees: super::JoshResult<HashSet<_>> = {
            let s = tracing::span!(
                tracing::Level::TRACE,
                "unapply filter",
                ?commit_message,
                ?rev,
                ?filtered_parent_ids,
                ?original_parents_refs
            );
            let _e = s.enter();
            original_parents_refs
                .iter()
                .map(|x| -> super::JoshResult<_> {
                    Ok(super::filters::unapply(
                        &repo,
                        &filterobj,
                        tree.clone(),
                        x.tree()?,
                    )?
                    .id())
                })
                .collect()
        };

        let new_trees = match new_trees {
            Ok(new_trees) => new_trees,
            Err(super::JoshError(msg)) => {
                return Err(super::josh_error(&format!(
                    "\nCan't apply {:?} ({:?})\n{}",
                    commit_message,
                    module_commit.id(),
                    msg
                )))
            }
        };

        let new_tree = match new_trees.len() {
            1 => repo.find_tree(
                *new_trees
                    .iter()
                    .next()
                    .ok_or(super::josh_error("iter.next"))?,
            )?,
            0 => {
                tracing::debug!("unrelated history");
                // 0 means the history is unrelated. Pushing it will fail if we are not
                // dealing with either a force push or a push with the "josh-merge" option set.
                super::filters::unapply(
                    &repo,
                    &filterobj,
                    tree,
                    empty_tree(&repo),
                )?
            }
            parent_count => {
                // This is a merge commit where the parents in the upstream repo
                // have differences outside of the current filter.
                // It is unclear what base tree to pick in this case.
                tracing::warn!("rejecting merge");
                return Ok(UnapplyFilter::RejectMerge(parent_count));
            }
        };

        ret =
            rewrite(&repo, &module_commit, &original_parents_refs, &new_tree)?;
        bm.insert(module_commit.id(), ret);
    }

    tracing::trace!("done {:?}", ret);
    return Ok(UnapplyFilter::Done(ret));
}

#[tracing::instrument(skip(repo, transaction))]
fn transform_commit(
    repo: &git2::Repository,
    filterobj: &filters::Filter,
    from_refsname: &str,
    to_refname: &str,
    transaction: &mut filter_cache::Transaction,
) -> super::JoshResult<usize> {
    let mut updated_count = 0;
    if let Ok(reference) = repo.revparse_single(&from_refsname) {
        let original_commit = reference.peel_to_commit()?;

        let filter_commit =
            super::history::walk(&repo, &filterobj, original_commit.id())?;

        transaction.insert(
            &super::filters::spec(filterobj),
            original_commit.id(),
            filter_commit,
        );

        let previous = repo
            .revparse_single(&to_refname)
            .map(|x| x.id())
            .unwrap_or(git2::Oid::zero());

        if filter_commit != previous {
            updated_count += 1;
            tracing::trace!(
                "transform_commit: update reference: {:?} -> {:?}, target: {:?}, filter: {:?}",
                &from_refsname,
                &to_refname,
                filter_commit,
                &super::filters::spec(&filterobj),
            );
        }

        if filter_commit != git2::Oid::zero() {
            ok_or!(
                repo.reference(
                    &to_refname,
                    filter_commit,
                    true,
                    "apply_filter"
                )
                .map(|_| ()),
                {
                    tracing::error!(
                        "can't update reference: {:?} -> {:?}, target: {:?}, filter: {:?}",
                        &from_refsname,
                        &to_refname,
                        filter_commit,
                        &super::filters::spec(&filterobj),
                    );
                }
            );
        }
    } else {
        tracing::warn!(
            "transform_commit: Can't find reference {:?}",
            &from_refsname
        );
    };
    return Ok(updated_count);
}

#[tracing::instrument(skip(repo))]
pub fn apply_filter_to_refs(
    repo: &git2::Repository,
    filterobj: &filters::Filter,
    refs: &[(String, String)],
) -> super::JoshResult<usize> {
    rs_tracing::trace_scoped!(
        "apply_filter_to_refs",
        "spec": super::filters::spec(&filterobj)
    );
    let mut transaction = super::filter_cache::Transaction::new();

    let mut updated_count = 0;
    for (k, v) in refs {
        updated_count +=
            transform_commit(&repo, &*filterobj, &k, &v, &mut transaction)?;
    }
    return Ok(updated_count);
}

fn select_parent_commits<'a>(
    original_commit: &'a git2::Commit,
    filtered_tree_id: git2::Oid,
    filtered_parent_commits: Vec<&'a git2::Commit>,
) -> Vec<&'a git2::Commit<'a>> {
    let affects_filtered = filtered_parent_commits
        .iter()
        .any(|x| filtered_tree_id != x.tree_id());

    let all_diffs_empty = original_commit
        .parents()
        .all(|x| x.tree_id() == original_commit.tree_id());

    return if affects_filtered || all_diffs_empty {
        filtered_parent_commits
    } else {
        vec![]
    };
}

fn is_empty_root(repo: &git2::Repository, tree: &git2::Tree) -> bool {
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

pub fn create_filtered_commit<'a>(
    repo: &'a git2::Repository,
    original_commmit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
) -> super::JoshResult<git2::Oid> {
    let is_initial_merge = filtered_parent_ids.len() > 1
        && !repo.merge_base_many(&filtered_parent_ids).is_ok();

    let filtered_parent_commits: std::result::Result<Vec<_>, _> =
        filtered_parent_ids
            .iter()
            .filter(|x| **x != git2::Oid::zero())
            .map(|x| repo.find_commit(*x))
            .collect();

    let mut filtered_parent_commits = filtered_parent_commits?;

    if is_initial_merge {
        filtered_parent_commits.retain(|x| x.tree_id() != empty_tree_id());
    }

    let selected_filtered_parent_commits: Vec<&_> = select_parent_commits(
        &original_commmit,
        filtered_tree.id(),
        filtered_parent_commits.iter().collect(),
    );

    if selected_filtered_parent_commits.len() == 0
        && !(original_commmit.parents().len() == 0
            && is_empty_root(&repo, &original_commmit.tree()?))
    {
        if filtered_parent_commits.len() != 0 {
            return Ok(filtered_parent_commits[0].id());
        }
        if filtered_tree.id() == empty_tree_id() {
            return Ok(git2::Oid::zero());
        }
    }

    return rewrite(
        &repo,
        &original_commmit,
        &selected_filtered_parent_commits,
        &filtered_tree,
    );
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
    rs_tracing::trace_scoped!("substract_tree X", "root": root);
    if let Some(cached) = cache.get(&(input, key)) {
        return Ok(repo.find_tree(*cached)?);
    }

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
    rs_tracing::trace_scoped!("substract fast");
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
