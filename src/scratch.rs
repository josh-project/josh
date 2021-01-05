use super::*;

// takes everything from base except it's tree and replaces it with the tree
// given
pub fn rewrite_commit(
    repo: &git2::Repository,
    base: &git2::Commit,
    parents: &[&git2::Commit],
    tree: &git2::Tree,
) -> JoshResult<git2::Oid> {
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

#[tracing::instrument(skip(repo))]
pub fn unapply_filter(
    repo: &git2::Repository,
    filterobj: &filters::Filter,
    unfiltered_old: git2::Oid,
    old: git2::Oid,
    new: git2::Oid,
) -> JoshResult<UnapplyFilter> {
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
        history::find_original(&repo, &mut bm, filterobj, unfiltered_old, new)?;
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
                .map(|x| -> JoshResult<_> {
                    history::find_original(
                        &repo,
                        &mut bm,
                        filterobj,
                        unfiltered_old,
                        *x,
                    )
                })
                .filter(|x| {
                    if let Ok(i) = x {
                        *i != git2::Oid::zero()
                    } else {
                        true
                    }
                })
                .map(|x| -> JoshResult<_> { Ok(repo.find_commit(x?)?) })
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

        let new_trees: JoshResult<std::collections::HashSet<_>> = {
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
                .map(|x| -> JoshResult<_> {
                    Ok(filters::unapply(
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
            Err(JoshError(msg)) => {
                return Err(josh_error(&format!(
                    "\nCan't apply {:?} ({:?})\n{}",
                    commit_message,
                    module_commit.id(),
                    msg
                )))
            }
        };

        let new_tree = match new_trees.len() {
            1 => repo.find_tree(
                *new_trees.iter().next().ok_or(josh_error("iter.next"))?,
            )?,
            0 => {
                tracing::debug!("unrelated history");
                // 0 means the history is unrelated. Pushing it will fail if we are not
                // dealing with either a force push or a push with the "josh-merge" option set.
                filters::unapply(&repo, &filterobj, tree, empty_tree(&repo))?
            }
            parent_count => {
                // This is a merge commit where the parents in the upstream repo
                // have differences outside of the current filter.
                // It is unclear what base tree to pick in this case.
                tracing::warn!("rejecting merge");
                return Ok(UnapplyFilter::RejectMerge(parent_count));
            }
        };

        ret = rewrite_commit(
            &repo,
            &module_commit,
            &original_parents_refs,
            &new_tree,
        )?;
        bm.insert(module_commit.id(), ret);
    }

    tracing::trace!("done {:?}", ret);
    return Ok(UnapplyFilter::Done(ret));
}

#[tracing::instrument(skip(repo))]
fn transform_commit(
    repo: &git2::Repository,
    filterobj: &filters::Filter,
    from_refsname: &str,
    to_refname: &str,
) -> JoshResult<usize> {
    let mut updated_count = 0;
    if let Ok(reference) = repo.revparse_single(&from_refsname) {
        let original_commit = reference.peel_to_commit()?;

        let filter_commit =
            filters::apply_to_commit(&repo, &filterobj, &original_commit)?;

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
                &filters::spec(&filterobj),
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
                        &filters::spec(&filterobj),
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
) -> JoshResult<usize> {
    rs_tracing::trace_scoped!(
        "apply_filter_to_refs",
        "spec": filters::spec(&filterobj)
    );

    let mut updated_count = 0;
    for (k, v) in refs {
        updated_count += transform_commit(&repo, &*filterobj, &k, &v)?;
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

pub fn create_filtered_commit<'a>(
    repo: &'a git2::Repository,
    original_commit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
    transaction: &mut filter_cache::Transaction,
    spec: &str,
) -> JoshResult<git2::Oid> {
    let r = create_filtered_commit2(
        repo,
        original_commit,
        filtered_parent_ids,
        filtered_tree,
    );

    let i = r.clone().unwrap_or(git2::Oid::zero());

    transaction.insert(spec, original_commit.id(), i);

    return r;
}

fn create_filtered_commit2<'a>(
    repo: &'a git2::Repository,
    original_commmit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
) -> JoshResult<git2::Oid> {
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
            && treeops::is_empty_root(&repo, &original_commmit.tree()?))
    {
        if filtered_parent_commits.len() != 0 {
            return Ok(filtered_parent_commits[0].id());
        }
        if filtered_tree.id() == empty_tree_id() {
            return Ok(git2::Oid::zero());
        }
    }

    return rewrite_commit(
        &repo,
        &original_commmit,
        &selected_filtered_parent_commits,
        &filtered_tree,
    );
}

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
