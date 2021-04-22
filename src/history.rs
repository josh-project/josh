use super::*;

pub fn walk2(
    filter: filter::Filter,
    input: git2::Oid,
    transaction: &cache::Transaction,
) -> JoshResult<()> {
    rs_tracing::trace_scoped!("walk2","spec":filter::spec(filter), "id": input.to_string());

    ok_or!(transaction.repo().find_commit(input), {
        return Ok(());
    });

    if transaction.known(filter, input) {
        return Ok(());
    }

    let (known, n_new) = find_known(filter, input, transaction)?;

    let walk = {
        let mut walk = transaction.repo().revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(input)?;
        for k in known.iter() {
            walk.hide(*k)?;
        }
        walk
    };

    log::info!(
        "Walking {} new commits for:\n{}\n",
        n_new,
        filter::pretty(filter, 4),
    );
    let mut n_commits = 0;
    let mut n_misses = transaction.misses();

    let walks = transaction.new_walk();

    for original_commit_id in walk {
        if !filter::apply_to_commit3(
            filter,
            &transaction.repo().find_commit(original_commit_id?)?,
            transaction,
        )? {
            break;
        }

        n_commits += 1;
        if n_commits % 1000 == 0 {
            log::debug!(
                "{} {} commits filtered, {} misses",
                " ->".repeat(walks),
                n_commits,
                transaction.misses() - n_misses,
            );
            n_misses = transaction.misses();
        }
    }

    log::info!(
        "{} {} commits filtered, {} misses",
        " ->".repeat(walks),
        n_commits,
        transaction.misses() - n_misses,
    );

    transaction.end_walk();

    return Ok(());
}

fn find_unapply_base(
    transaction: &cache::Transaction,
    bm: &mut std::collections::HashMap<git2::Oid, git2::Oid>,
    filter: filter::Filter,
    contained_in: git2::Oid,
    filtered: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    if contained_in == git2::Oid::zero() {
        return Ok(git2::Oid::zero());
    }
    if let Some(original) = bm.get(&filtered) {
        return Ok(*original);
    }
    let contained_in_commit = transaction.repo().find_commit(contained_in)?;
    let oid = filter::apply_to_commit(filter, &contained_in_commit, transaction)?;
    if oid != git2::Oid::zero() {
        bm.insert(contained_in, oid);
    }
    let mut walk = transaction.repo().revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(contained_in)?;

    for original in walk {
        let original = transaction.repo().find_commit(original?)?;
        if filtered == filter::apply_to_commit(filter, &original, transaction)? {
            bm.insert(filtered, original.id());
            return Ok(original.id());
        }
    }

    return Ok(git2::Oid::zero());
}

pub fn find_original(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    contained_in: git2::Oid,
    filtered: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    if contained_in == git2::Oid::zero() {
        return Ok(git2::Oid::zero());
    }
    let mut walk = transaction.repo().revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(contained_in)?;

    for original in walk {
        let original = transaction.repo().find_commit(original?)?;
        if filtered == filter::apply_to_commit(filter, &original, transaction)? {
            if original.parent_ids().count() == 1 {
                let fp = filter::apply_to_commit(
                    filter,
                    &original.parents().next().unwrap(),
                    transaction,
                )?;

                if fp == filtered {
                    continue;
                }
            }
            return Ok(original.id());
        }
    }

    return Ok(git2::Oid::zero());
}

fn find_known(
    filter: filter::Filter,
    input: git2::Oid,
    transaction: &cache::Transaction,
) -> JoshResult<(Vec<git2::Oid>, usize)> {
    log::debug!("find_known");
    let mut known = vec![];
    let mut walk = transaction.repo().revwalk()?;
    walk.push(input)?;

    let n_new = walk
        .with_hide_callback(&|id| {
            let k = transaction.known(filter, id);
            if k {
                known.push(id)
            }
            k
        })?
        .count();
    log::debug!("/find_known {}", n_new);
    return Ok((known, n_new));
}

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

    let b = repo.commit_create_buffer(
        &base.author(),
        &base.committer(),
        &base.message_raw().unwrap_or("no message"),
        tree,
        parents,
    )?;

    return Ok(repo.odb()?.write(git2::ObjectType::Commit, &b)?);
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

#[tracing::instrument(skip(transaction))]
pub fn unapply_filter(
    transaction: &cache::Transaction,
    filterobj: filter::Filter,
    original_target: git2::Oid,
    old: git2::Oid,
    new: git2::Oid,
    keep_orphans: bool,
    reparent_orphans: Option<git2::Oid>,
    amends: &std::collections::HashMap<String, git2::Oid>,
) -> JoshResult<UnapplyResult> {
    let mut bm = std::collections::HashMap::new();
    let mut ret = original_target;

    let walk = {
        let mut walk = transaction.repo().revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(new)?;
        if let Ok(_) = walk.hide(old) {
            tracing::trace!("walk: hidden {}", old);
        } else {
            tracing::warn!("walk: can't hide");
        }
        walk
    };

    for rev in walk {
        let rev = rev?;

        let s = tracing::span!(tracing::Level::TRACE, "walk commit", ?rev);
        let _e = s.enter();

        let module_commit = transaction.repo().find_commit(rev)?;

        if bm.contains_key(&module_commit.id()) {
            continue;
        }

        let mut filtered_parent_ids: Vec<_> = module_commit.parent_ids().collect();

        let is_initial_merge = filtered_parent_ids.len() == 2
            && !transaction
                .repo()
                .merge_base_many(&filtered_parent_ids)
                .is_ok();

        if !keep_orphans && is_initial_merge {
            filtered_parent_ids.pop();
        }

        let original_parents: std::result::Result<Vec<_>, _> = filtered_parent_ids
            .iter()
            .map(|x| -> JoshResult<_> {
                find_unapply_base(&transaction, &mut bm, filterobj, original_target, *x)
            })
            .filter(|x| {
                if let Ok(i) = x {
                    *i != git2::Oid::zero()
                } else {
                    true
                }
            })
            .map(|x| -> JoshResult<_> { Ok(transaction.repo().find_commit(x?)?) })
            .collect();

        let mut original_parents = original_parents?;

        if let (0, Some(reparent)) = (original_parents.len(), reparent_orphans) {
            original_parents = vec![transaction.repo().find_commit(reparent)?];
        }
        tracing::info!(
            "parents: {:?} -> {:?}",
            original_parents,
            filtered_parent_ids
        );

        let original_parents_refs: Vec<&git2::Commit> = original_parents.iter().collect();

        let tree = module_commit.tree()?;

        let commit_message = module_commit.summary().unwrap_or("NO COMMIT MESSAGE");

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
                    Ok(filter::unapply(transaction, filterobj, tree.clone(), x.tree()?)?.id())
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
            1 => transaction
                .repo()
                .find_tree(*new_trees.iter().next().ok_or(josh_error("iter.next"))?)?,
            0 => {
                tracing::debug!("unrelated history");
                // 0 means the history is unrelated. Pushing it will fail if we are not
                // dealing with either a force push or a push with the "merge" option set.
                filter::unapply(
                    transaction,
                    filterobj,
                    tree,
                    filter::tree::empty(&transaction.repo()),
                )?
            }
            parent_count => {
                // This is a merge commit where the parents in the upstream repo
                // have differences outside of the current filter.
                // It is unclear what base tree to pick in this case.
                tracing::warn!("rejecting merge");
                return Ok(UnapplyResult::RejectMerge(parent_count));
            }
        };

        ret = rewrite_commit(
            &transaction.repo(),
            &module_commit,
            &original_parents_refs,
            &new_tree,
        )?;

        if let Some(id) = super::get_change_id(&module_commit) {
            if let Some(commit_id) = amends.get(&id) {
                let mut merged_index = transaction.repo().merge_commits(
                    &transaction.repo().find_commit(*commit_id)?,
                    &transaction.repo().find_commit(ret)?,
                    Some(git2::MergeOptions::new().file_favor(git2::FileFavor::Theirs)),
                )?;

                if merged_index.has_conflicts() {
                    return Ok(UnapplyResult::RejectAmend(
                        module_commit
                            .summary()
                            .unwrap_or("<no message>")
                            .to_string(),
                    ));
                }

                let merged_tree = merged_index.write_tree_to(&transaction.repo())?;

                ret = rewrite_commit(
                    &transaction.repo(),
                    &module_commit,
                    &original_parents_refs,
                    &transaction.repo().find_tree(merged_tree)?,
                )?;
            }
        }

        bm.insert(module_commit.id(), ret);
    }

    tracing::trace!("done {:?}", ret);
    return Ok(UnapplyResult::Done(ret));
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
    original_commit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
    transaction: &cache::Transaction,
    filter: filter::Filter,
) -> JoshResult<git2::Oid> {
    let (r, is_new) = create_filtered_commit2(
        &transaction.repo(),
        original_commit,
        filtered_parent_ids,
        filtered_tree,
    )?;

    let store = is_new || original_commit.parent_ids().len() != 1;

    transaction.insert(filter, original_commit.id(), r, store);

    return Ok(r);
}

fn create_filtered_commit2<'a>(
    repo: &'a git2::Repository,
    original_commmit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
) -> JoshResult<(git2::Oid, bool)> {
    let filtered_parent_commits: std::result::Result<Vec<_>, _> = filtered_parent_ids
        .iter()
        .filter(|x| **x != git2::Oid::zero())
        .map(|x| repo.find_commit(*x))
        .collect();

    let mut filtered_parent_commits = filtered_parent_commits?;

    if filtered_parent_commits
        .iter()
        .any(|x| x.tree_id() == filter::tree::empty_id())
    {
        let is_initial_merge =
            filtered_parent_ids.len() > 1 && !repo.merge_base_many(&filtered_parent_ids).is_ok();

        if is_initial_merge {
            filtered_parent_commits.retain(|x| x.tree_id() != filter::tree::empty_id());
        }
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
            return Ok((filtered_parent_commits[0].id(), false));
        }
        if filtered_tree.id() == filter::tree::empty_id() {
            return Ok((git2::Oid::zero(), false));
        }
    }

    return Ok((
        rewrite_commit(
            &repo,
            &original_commmit,
            &selected_filtered_parent_commits,
            &filtered_tree,
        )?,
        true,
    ));
}

fn is_empty_root(repo: &git2::Repository, tree: &git2::Tree) -> bool {
    if tree.id() == filter::tree::empty_id() {
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
