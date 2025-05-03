use super::*;
use std::collections::HashMap;

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
        if filter::is_linear(filter) {
            walk.simplify_first_parent()?;
        }
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

    Ok(())
}

fn find_unapply_base(
    transaction: &cache::Transaction,
    // Used as a cache to avoid re-applying the filter to the same commit -
    // this function is called during revwalk so there be a lot of repeated
    // calls
    filtered_to_original: &mut HashMap<git2::Oid, git2::Oid>,
    filter: filter::Filter,
    // When building the filtered_to_original mapping use this as a starting point
    // for the search for originals. If there are multiple originals that map to the
    // same filtered commit (which is common) use one that is reachable from contained_in.
    // Or, in other words, one that is contained in the history of contained_in.
    contained_in: git2::Oid,
    // Filtered OID to compare against
    filtered: git2::Oid,
) -> JoshResult<git2::Oid> {
    if contained_in == git2::Oid::zero() {
        tracing::info!("contained in zero",);
        return Ok(git2::Oid::zero());
    }

    if let Some(original) = filtered_to_original.get(&filtered) {
        tracing::info!("Found in filtered_to_original",);
        return Ok(*original);
    }

    let contained_in_commit = transaction.repo().find_commit(contained_in)?;
    let oid = filter::apply_to_commit(filter, &contained_in_commit, transaction)?;
    if oid != git2::Oid::zero() {
        filtered_to_original.insert(oid, contained_in);
    }

    // Start a revwalk in the original history tree starting from the
    // contained_in "hint"
    let mut walk = transaction.repo().revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(contained_in)?;

    for original in walk {
        let original = transaction.repo().find_commit(original?)?;
        if filtered == filter::apply_to_commit(filter, &original, transaction)? {
            // In case a match is found, cache the result
            filtered_to_original.insert(filtered, original.id());
            tracing::info!("found original properly {}", original.id());
            return Ok(original.id());
        }
    }

    tracing::info!("Didn't find original",);
    Ok(git2::Oid::zero())
}

pub fn find_original(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    contained_in: git2::Oid,
    filtered: git2::Oid,
    linear: bool,
) -> JoshResult<git2::Oid> {
    if contained_in == git2::Oid::zero() {
        return Ok(git2::Oid::zero());
    }
    if filter == filter::nop() {
        return Ok(filtered);
    }
    let mut walk = transaction.repo().revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    if linear {
        walk.simplify_first_parent()?;
    }
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

    Ok(git2::Oid::zero())
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
        .with_hide_callback(&mut |id| {
            let k = transaction.known(filter, id);
            if k {
                known.push(id)
            }
            k
        })?
        .count();
    log::debug!("/find_known {}", n_new);
    Ok((known, n_new))
}

pub struct RewriteData<'a> {
    pub tree: git2::Tree<'a>,
    pub author: Option<(String, String)>,
    pub committer: Option<(String, String)>,
    pub message: Option<String>,
}

// takes everything from base except its tree and replaces it with the tree
// given
pub fn rewrite_commit(
    repo: &git2::Repository,
    base: &git2::Commit,
    parents: &[&git2::Commit],
    rewrite_data: RewriteData,
    unsign: bool,
) -> JoshResult<git2::Oid> {
    let odb = repo.odb()?;
    let odb_commit = odb.read(base.id())?;
    assert!(odb_commit.kind() == git2::ObjectType::Commit);

    // gix_object uses byte strings for Oids, but in hex representation, not raw bytes. Its `Format` implementation
    // writes out hex-encoded bytes. Because CommitRef's reference lifetimes we have to this, before creating CommitRef
    let tree_id = format!("{}", rewrite_data.tree.id());
    let parent_ids = parents
        .iter()
        .map(|x| format!("{}", x.id()))
        .collect::<Vec<_>>();

    let mut commit = gix_object::CommitRef::from_bytes(odb_commit.data())?;

    commit.tree = tree_id.as_bytes().into();

    commit.parents.clear();
    commit
        .parents
        .extend(parent_ids.iter().map(|x| x.as_bytes().into()));

    if let Some(ref msg) = rewrite_data.message {
        commit.message = msg.as_bytes().into();
    }

    if let Some((ref author, ref email)) = rewrite_data.author {
        commit.author.name = author.as_bytes().into();
        commit.author.email = email.as_bytes().into();
    }

    if let Some((ref author, ref email)) = rewrite_data.committer {
        commit.committer.name = author.as_bytes().into();
        commit.committer.email = email.as_bytes().into();
    }

    commit
        .extra_headers
        .retain(|(k, _)| *k != "gpgsig".as_bytes() || !unsign);

    let mut b = vec![];
    gix_object::WriteTo::write_to(&commit, &mut b)?;

    return Ok(odb.write(git2::ObjectType::Commit, &b)?);
}

// Given an OID of an unfiltered commit and a filter,
// find the oldest commit (within the topological order)
// that gives the same result when filtered
fn find_oldest_similar_commit(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    unfiltered: git2::Oid,
) -> JoshResult<git2::Oid> {
    let walk = {
        let mut walk = transaction.repo().revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        walk.push(unfiltered)?;
        walk
    };
    tracing::info!("oldest similar?");
    let unfiltered_commit = transaction.repo().find_commit(unfiltered)?;
    let filtered = filter::apply_to_commit(filter, &unfiltered_commit, transaction)?;
    let mut prev_rev = unfiltered;
    for rev in walk {
        let rev = rev?;
        tracing::info!("next");
        let rev_commit = transaction.repo().find_commit(rev)?;
        if filtered != filter::apply_to_commit(filter, &rev_commit, transaction)? {
            tracing::info!("diff! {}", prev_rev);
            return Ok(prev_rev);
        }
        prev_rev = rev;
    }
    tracing::info!("bottom");
    Ok(prev_rev)
}

fn find_new_branch_base(
    transaction: &cache::Transaction,
    filtered_to_original: &mut HashMap<git2::Oid, git2::Oid>,
    filter: filter::Filter,
    // See "contained_in" in find_unapply_base
    contained_in: git2::Oid,
    filtered: git2::Oid,
) -> JoshResult<git2::Oid> {
    let walk = {
        let mut walk = transaction.repo().revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        walk.push(filtered)?;
        walk
    };
    tracing::info!("new branch base?");

    // Walk filtered history, trying to find a base for every commit
    for rev in walk {
        let rev = rev?;
        if let Ok(base) =
            find_unapply_base(transaction, filtered_to_original, filter, contained_in, rev)
        {
            if base != git2::Oid::zero() {
                tracing::info!("new branch base: {:?} mapping to {:?}", base, rev);
                let base =
                    if let Ok(new_base) = find_oldest_similar_commit(transaction, filter, base) {
                        new_base
                    } else {
                        base
                    };

                tracing::info!("inserting in filtered_to_original {}, {}", rev, base);
                filtered_to_original.insert(rev, base);

                return Ok(rev);
            }
        }
    }
    tracing::info!("new branch base not found");
    Ok(git2::Oid::zero())
}

#[tracing::instrument(skip(transaction, change_ids))]
pub fn unapply_filter(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    original_target: git2::Oid,
    old_filtered_oid: git2::Oid,
    new_filtered_oid: git2::Oid,
    keep_orphans: bool,
    reparent_orphans: Option<git2::Oid>,
    change_ids: &mut Option<Vec<Change>>,
) -> JoshResult<git2::Oid> {
    let mut filtered_to_original = HashMap::new();
    let mut ret = original_target;

    let old_filtered_oid = if old_filtered_oid == git2::Oid::zero() {
        match find_new_branch_base(
            transaction,
            &mut filtered_to_original,
            filter,
            original_target,
            new_filtered_oid,
        ) {
            Ok(res) => {
                tracing::info!("No error, branch base {} ", res);
                res
            }
            Err(_) => {
                tracing::info!("Error in new branch base");
                old_filtered_oid
            }
        }
    } else {
        tracing::info!("Old not zero");
        old_filtered_oid
    };

    if new_filtered_oid == old_filtered_oid {
        tracing::info!("New == old. Pushing a new branch?");

        let unapply_result = if let Some(original) = filtered_to_original.get(&new_filtered_oid) {
            tracing::info!("Found in filtered_to_original {}", original);
            *original
        } else {
            tracing::info!("Had to go through the whole thing",);
            find_original(
                transaction,
                filter,
                original_target,
                new_filtered_oid,
                false,
            )?
        };

        return Ok(unapply_result);
    }

    tracing::info!("before walk");

    let walk = {
        let mut walk = transaction.repo().revwalk()?;

        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(new_filtered_oid)?;

        // The main reason hide() can fail is if old_filtered_oid is not found in the repo
        if walk.hide(old_filtered_oid).is_ok() {
            tracing::info!("walk: hidden {}", old_filtered_oid);
        } else {
            tracing::warn!("walk: can't hide");
        }

        walk
    };

    // Walk starting from new filtered OID
    for rev in walk {
        let rev = rev?;

        let span = tracing::span!(tracing::Level::TRACE, "walk commit", ?rev);
        let _span_guard = span.enter();

        tracing::info!("walk commit: {:?}", rev);
        let module_commit = transaction.repo().find_commit(rev)?;

        if filtered_to_original.contains_key(&module_commit.id()) {
            continue;
        }

        let mut filtered_parent_ids: Vec<_> = module_commit.parent_ids().collect();
        let is_initial_merge = filtered_parent_ids.len() == 2
            && transaction
                .repo()
                .merge_base_many(&filtered_parent_ids)
                .is_err();

        if !keep_orphans && is_initial_merge {
            filtered_parent_ids.pop();
        }

        // For every parent of a filtered commit, find unapply base
        let original_parents: Result<Vec<_>, _> = filtered_parent_ids
            .iter()
            .map(|filtered_parent_id| -> JoshResult<_> {
                find_unapply_base(
                    transaction,
                    &mut filtered_to_original,
                    filter,
                    original_target,
                    *filtered_parent_id,
                )
            })
            .filter(|unapply_base| {
                if let Ok(oid) = unapply_base {
                    *oid != git2::Oid::zero()
                } else {
                    true
                }
            })
            .map(|unapply_base| -> JoshResult<_> {
                Ok(transaction.repo().find_commit(unapply_base?)?)
            })
            .collect();

        // If there are no parents and "reparent" option is given, use the given OID as a parent
        let mut original_parents = original_parents?;
        if let (0, Some(reparent)) = (original_parents.len(), reparent_orphans) {
            original_parents = vec![transaction.repo().find_commit(reparent)?];
        }

        tracing::info!(
            "parents: {:?} -> {:?}",
            original_parents,
            filtered_parent_ids
        );

        // Convert original_parents to a vector of (rust) references
        let original_parents: Vec<&git2::Commit> = original_parents.iter().collect();
        let tree = module_commit.tree()?;
        let commit_message = module_commit.summary().unwrap_or("NO COMMIT MESSAGE");

        let new_trees: JoshResult<Vec<_>> = {
            let span = tracing::span!(
                tracing::Level::TRACE,
                "unapply filter",
                ?commit_message,
                ?rev,
                ?filtered_parent_ids,
                ?original_parents
            );
            let _span_guard = span.enter();

            original_parents
                .iter()
                .map(|commit| -> JoshResult<_> {
                    Ok(filter::unapply(transaction, filter, tree.clone(), commit.tree()?)?.id())
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
                )));
            }
        };

        let new_unique_trees = {
            let mut new_trees_dedup = new_trees.clone();
            new_trees_dedup.sort();
            new_trees_dedup.dedup();
            new_trees_dedup.len()
        };

        let new_tree = match new_unique_trees {
            // The normal case: Either there was only one parent or all of them where the same
            // outside of the current filter in which case they collapse into one tree and that
            // is the one we pick
            1 => transaction.repo().find_tree(new_trees[0])?,

            // 0 means the history is unrelated. Pushing it will fail if we are not
            // dealing with either a force push or a push with the "merge" option set.
            0 => {
                tracing::debug!("unrelated history");
                filter::unapply(
                    transaction,
                    filter,
                    tree,
                    filter::tree::empty(transaction.repo()),
                )?
            }

            // This will typically be parent_count == 2 and mean we are dealing with a merge
            // where the parents have differences outside of the filter.
            parent_count => {
                let mut tid = git2::Oid::zero();
                for i in 0..parent_count {
                    // If one of the parents is a descendant of the target branch and the other is
                    // not, pick the tree of the one that is a descendant.
                    if (original_parents[i].id() == original_target)
                        || transaction
                            .repo()
                            .graph_descendant_of(original_parents[i].id(), original_target)?
                    {
                        tid = new_trees[i];
                        break;
                    }
                }

                if tid == git2::Oid::zero() && parent_count == 2 {
                    // If we could not select one of the parents, try to merge them.
                    // We expect conflicts to occur only in the paths that are present in
                    // the filtered commit.
                    // As we are going to replace the contents of these files with commit being
                    // pushed, we can ignore those conflicts. To do that we perform the merge
                    // twice: Once with the "ours" and once with the "theirs" merge file favor.
                    // After that we do "unapply()" on both resulting trees, which will replace
                    // the files selected by the filter with the content being pushed.
                    // If our assumption was correct and all conflicts were in filtered files,
                    // both resulting trees will be the same and we can pick the result to proceed.

                    let mut mergeopts = git2::MergeOptions::new();
                    mergeopts.file_favor(git2::FileFavor::Ours);

                    let mut merged_index = transaction.repo().merge_commits(
                        original_parents[0],
                        original_parents[1],
                        Some(&mergeopts),
                    )?;
                    let base_tree = merged_index.write_tree_to(transaction.repo())?;
                    let tid_ours = filter::unapply(
                        transaction,
                        filter,
                        tree.clone(),
                        transaction.repo().find_tree(base_tree)?,
                    )?
                    .id();

                    mergeopts.file_favor(git2::FileFavor::Theirs);

                    let mut merged_index = transaction.repo().merge_commits(
                        original_parents[0],
                        original_parents[1],
                        Some(&mergeopts),
                    )?;
                    let base_tree = merged_index.write_tree_to(transaction.repo())?;
                    let tid_theirs = filter::unapply(
                        transaction,
                        filter,
                        tree.clone(),
                        transaction.repo().find_tree(base_tree)?,
                    )?
                    .id();

                    if tid_ours == tid_theirs {
                        tid = tid_ours;
                    }
                }

                if tid == git2::Oid::zero() {
                    // We give up. If we see this message again we need to investigate once
                    // more and maybe consider allowing a manual override as last resort.
                    tracing::warn!("rejecting merge");
                    let msg = format!(
                        "rejecting merge with {} parents:\n{:?} ({:?})\n1) {:?} ({:?})\n2) {:?} ({:?})",
                        parent_count,
                        module_commit.summary().unwrap_or_default(),
                        module_commit.id(),
                        original_parents[0].summary().unwrap_or_default(),
                        original_parents[0].id(),
                        original_parents[1].summary().unwrap_or_default(),
                        original_parents[1].id(),
                    );
                    return Err(josh_error(&msg));
                }

                transaction.repo().find_tree(tid)?
            }
        };

        ret = rewrite_commit(
            transaction.repo(),
            &module_commit,
            &original_parents,
            RewriteData {
                tree: new_tree.clone(),
                author: None,
                committer: None,
                message: None,
            },
            false,
        )?;

        ret = if original_parents.len() == 1
            && new_tree.id() == original_parents[0].tree_id()
            && Some(module_commit.tree_id()) != module_commit.parents().next().map(|x| x.tree_id())
        {
            original_parents[0].id()
        } else {
            if let Some(change_ids) = change_ids {
                change_ids.push(get_change_id(&module_commit, ret));
            }
            ret
        };

        filtered_to_original.insert(module_commit.id(), ret);
    }

    tracing::trace!("done {:?}", ret);
    Ok(ret)
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

    if affects_filtered || all_diffs_empty {
        filtered_parent_commits
    } else {
        vec![]
    }
}

pub fn remove_commit_signature<'a>(
    original_commit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
    transaction: &cache::Transaction,
    filter: filter::Filter,
) -> JoshResult<git2::Oid> {
    let (r, is_new) = create_filtered_commit2(
        transaction.repo(),
        original_commit,
        filtered_parent_ids,
        RewriteData {
            tree: filtered_tree,
            author: None,
            committer: None,
            message: None,
        },
        true,
    )?;

    let store = is_new || original_commit.parent_ids().len() != 1;

    transaction.insert(filter, original_commit.id(), r, store);

    Ok(r)
}

pub fn drop_commit<'a>(
    original_commit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    transaction: &cache::Transaction,
    filter: filter::Filter,
) -> JoshResult<git2::Oid> {
    let r = if let Some(id) = filtered_parent_ids.first() {
        *id
    } else {
        git2::Oid::zero()
    };

    transaction.insert(filter, original_commit.id(), r, false);

    Ok(r)
}

pub fn create_filtered_commit<'a>(
    original_commit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    rewrite_data: RewriteData,
    transaction: &cache::Transaction,
    filter: filter::Filter,
) -> JoshResult<git2::Oid> {
    let (r, is_new) = create_filtered_commit2(
        transaction.repo(),
        original_commit,
        filtered_parent_ids,
        rewrite_data,
        false,
    )?;

    let store = is_new || original_commit.parent_ids().len() != 1;

    transaction.insert(filter, original_commit.id(), r, store);

    Ok(r)
}

fn create_filtered_commit2<'a>(
    repo: &'a git2::Repository,
    original_commit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    rewrite_data: RewriteData,
    unsign: bool,
) -> JoshResult<(git2::Oid, bool)> {
    let filtered_parent_commits: Result<Vec<_>, _> = filtered_parent_ids
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
            filtered_parent_ids.len() > 1 && repo.merge_base_many(&filtered_parent_ids).is_err();

        if is_initial_merge {
            filtered_parent_commits.retain(|x| x.tree_id() != filter::tree::empty_id());
        }
    }

    let selected_filtered_parent_commits: Vec<&_> = select_parent_commits(
        original_commit,
        rewrite_data.tree.id(),
        filtered_parent_commits.iter().collect(),
    );

    if selected_filtered_parent_commits.is_empty()
        && !(original_commit.parents().len() == 0 && is_empty_root(repo, &original_commit.tree()?))
    {
        if !filtered_parent_commits.is_empty() {
            return Ok((filtered_parent_commits[0].id(), false));
        }
        if rewrite_data.tree.id() == filter::tree::empty_id() {
            return Ok((git2::Oid::zero(), false));
        }
    }

    Ok((
        rewrite_commit(
            repo,
            original_commit,
            &selected_filtered_parent_commits,
            rewrite_data,
            unsign,
        )?,
        true,
    ))
}

fn is_empty_root(repo: &git2::Repository, tree: &git2::Tree) -> bool {
    if tree.id() == filter::tree::empty_id() {
        return true;
    }

    let mut all_empty = true;

    for e in tree.iter() {
        if let Ok(Ok(t)) = e.to_object(repo).map(|x| x.into_tree()) {
            all_empty = all_empty && is_empty_root(repo, &t);
        } else {
            return false;
        }
    }
    all_empty
}
