use super::*;
use compat::Git2CompatExt;
use compat::GitOxideCompatExt;
use gix::bstr::{BString, ByteSlice};

pub fn walk2(
    filter: filter::Filter,
    input: git2::Oid,
    transaction: &cache::Transaction,
) -> JoshResult<()> {
    rs_tracing::trace_scoped!("walk2", "spec": filter::spec(filter), "id": input.to_string());

    ok_or!(transaction.repo().find_commit(input), {
        return Ok(());
    });

    if transaction.known(filter, input) {
        return Ok(());
    }

    let known = find_known(filter, input.to_oxide(), transaction)?;

    let walk = {
        let mut walk = transaction.repo().revwalk()?;
        if filter::is_linear(filter) {
            walk.simplify_first_parent()?;
        }
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(input)?;
        for known_id in known.iter() {
            walk.hide(known_id.to_git2())?;
        }
        walk
    };

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
    bm: &mut HashMap<git2::Oid, git2::Oid>,
    filter: filter::Filter,
    contained_in: git2::Oid,
    filtered: git2::Oid,
) -> JoshResult<git2::Oid> {
    if contained_in == git2::Oid::zero() {
        tracing::info!("contained in zero",);
        return Ok(git2::Oid::zero());
    }
    if let Some(original) = bm.get(&filtered) {
        tracing::info!("Found in bm",);
        return Ok(*original);
    }
    let contained_in_commit = transaction.repo().find_commit(contained_in)?;
    let oid = filter::apply_to_commit(filter, &contained_in_commit, transaction)?;
    if oid != git2::Oid::zero() {
        bm.insert(oid, contained_in);
    }
    let mut walk = transaction.repo().revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(contained_in)?;

    for original in walk {
        let original = transaction.repo().find_commit(original?)?;
        if filtered == filter::apply_to_commit(filter, &original, transaction)? {
            bm.insert(filtered, original.id());
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
    input: gix::ObjectId,
    transaction: &cache::Transaction,
) -> JoshResult<Vec<gix::ObjectId>> {
    let walk = transaction.oxide_repo().rev_walk([input]).all()?;

    walk.filter_map(|id| -> Option<JoshResult<gix::ObjectId>> {
        let id = match id {
            Err(e) => return Some(Err(josh_error(&format!("error while rev walk: {}", e)))),
            Ok(id) => id.detach(),
        };

        let id_git2 = id.to_git2();
        if transaction.known(filter, id_git2) {
            Some(Ok(id))
        } else {
            None
        }
    })
    .collect::<JoshResult<Vec<_>>>()
}

// fn find_known(
//     filter: filter::Filter,
//     input: git2::Oid,
//     transaction: &cache::Transaction,
// ) -> JoshResult<(Vec<git2::Oid>, usize)> {
//     log::debug!("find_known");
//     let mut known = vec![];
//     let mut walk = transaction.repo().revwalk()?;
//     walk.push(input)?;
//
//     let n_new = walk
//         .with_hide_callback(&|id| {
//             let k = transaction.known(filter, id);
//             if k {
//                 known.push(id)
//             }
//             k
//         })?
//         .count();
//     log::debug!("/find_known {}", n_new);
//     Ok((known, n_new))
// }

pub fn rewrite_commit(
    repo: &gix::Repository,
    base: &gix::Commit,
    parents: &[gix::ObjectId],
    tree: gix::ObjectId,
    author: Option<(String, String)>,
    message: Option<String>,
    _unsign: bool,
) -> JoshResult<gix::ObjectId> {
    let message = message.unwrap_or(
        base.message_raw()
            .unwrap_or(BString::from("no message").as_bstr())
            .to_string(),
    );

    let (author, committer) = if let Some((author, email)) = author {
        let base_author_time = base.author()?.time;
        let new_author = gix::actor::Signature {
            name: BString::from(author.as_str()),
            email: BString::from(email.as_str()),
            time: base_author_time,
        };

        let base_committer_time = base.committer()?.time;
        let new_committer = gix::actor::Signature {
            name: BString::from(author),
            email: BString::from(email),
            time: base_committer_time,
        };

        (new_author, new_committer)
    } else {
        (base.author()?.to_owned(), base.committer()?.to_owned())
    };

    let commit = gix_object::Commit {
        tree,
        parents: parents.into_iter().map(|&id| id.clone()).collect(),
        author,
        committer,
        encoding: None,
        message: BString::from(message),
        extra_headers: vec![],
    };

    let oid = repo.write_object(commit)?;
    Ok(oid.detach())
}

// takes everything from base except it's tree and replaces it with the tree
// given
// pub fn rewrite_commit(
//     repo: &git2::Repository,
//     base: &git2::Commit,
//     parents: &[&git2::Commit],
//     tree: &git2::Tree,
//     author: Option<(String, String)>,
//     message: Option<String>,
//     unsign: bool,
// ) -> JoshResult<git2::Oid> {
//     let message = message.unwrap_or(base.message_raw().unwrap_or("no message").to_string());
//
//     let b = if let Some((author, email)) = author {
//         let a = base.author();
//         let new_a = git2::Signature::new(&author, &email, &a.when())?;
//         let c = base.committer();
//         let new_c = git2::Signature::new(&author, &email, &c.when())?;
//         repo.commit_create_buffer(&new_a, &new_c, &message, tree, parents)?
//     } else {
//         repo.commit_create_buffer(&base.author(), &base.committer(), &message, tree, parents)?
//     };
//
//     if let (false, Ok((sig, _))) = (unsign, repo.extract_signature(&base.id(), None)) {
//         // Re-create the object with the original signature (which of course does not match any
//         // more, but this is needed to guarantee perfect round-trips).
//         let b = b
//             .as_str()
//             .ok_or_else(|| josh_error("non-UTF-8 signed commit"))?;
//         let sig = sig
//             .as_str()
//             .ok_or_else(|| josh_error("non-UTF-8 signature"))?;
//         return Ok(repo.commit_signed(b, sig, None)?);
//     }
//
//     return Ok(repo.odb()?.write(git2::ObjectType::Commit, &b)?);
// }

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
    bm: &mut HashMap<git2::Oid, git2::Oid>,
    filter: filter::Filter,
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

    for rev in walk {
        let rev = rev?;
        if let Ok(base) = find_unapply_base(transaction, bm, filter, contained_in, rev) {
            if base != git2::Oid::zero() {
                tracing::info!("new branch base: {:?} mapping to {:?}", base, rev);
                let base =
                    if let Ok(new_base) = find_oldest_similar_commit(transaction, filter, base) {
                        new_base
                    } else {
                        base
                    };
                tracing::info!("inserting in bm {}, {}", rev, base);
                bm.insert(rev, base);
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
    filterobj: filter::Filter,
    original_target: git2::Oid,
    old: git2::Oid,
    new: git2::Oid,
    keep_orphans: bool,
    reparent_orphans: Option<git2::Oid>,
    change_ids: &mut Option<Vec<Change>>,
) -> JoshResult<git2::Oid> {
    let mut bm = HashMap::new();
    let mut ret = original_target;

    let old = if old == git2::Oid::zero() {
        match find_new_branch_base(transaction, &mut bm, filterobj, original_target, new) {
            Ok(res) => {
                tracing::info!("No error, branch base {} ", res);
                res
            }
            Err(_) => {
                tracing::info!("Error in new branch base");
                old
            }
        }
    } else {
        tracing::info!("Old not zero");
        old
    };

    if new == old {
        tracing::info!("New == old. Pushing a new branch?");
        let ret = if let Some(original) = bm.get(&new) {
            tracing::info!("Found in bm {}", original);
            *original
        } else {
            tracing::info!("Had to go through the whole thing",);
            find_original(transaction, filterobj, original_target, new, false)?
        };
        return Ok(ret);
    }

    tracing::info!("before walk");

    let walk = {
        let mut walk = transaction.repo().revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(new)?;
        if walk.hide(old).is_ok() {
            tracing::info!("walk: hidden {}", old);
        } else {
            tracing::warn!("walk: can't hide");
        }
        walk
    };

    for rev in walk {
        let rev = rev?;

        let s = tracing::span!(tracing::Level::TRACE, "walk commit", ?rev);
        let _e = s.enter();
        tracing::info!("walk commit: {:?}", rev);

        let module_commit = transaction.repo().find_commit(rev)?;

        if bm.contains_key(&module_commit.id()) {
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

        let original_parents: Result<Vec<_>, _> = filtered_parent_ids
            .iter()
            .map(|x| -> JoshResult<_> {
                find_unapply_base(transaction, &mut bm, filterobj, original_target, *x)
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

        let new_trees: JoshResult<Vec<_>> = {
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

        let mut dedup = new_trees.clone();
        dedup.sort();
        dedup.dedup();

        let new_tree = match dedup.len() {
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
                    filterobj,
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
                    if (original_parents_refs[i].id() == original_target)
                        || transaction
                            .repo()
                            .graph_descendant_of(original_parents_refs[i].id(), original_target)?
                    {
                        tid = new_trees[i];
                        break;
                    }
                }

                if tid == git2::Oid::zero() && parent_count == 2 {
                    // If we could not select one of the parents, try to merge them.

                    if let Ok(mut merged_index) = transaction.repo().merge_commits(
                        original_parents_refs[0],
                        original_parents_refs[1],
                        None,
                    ) {
                        // If we can auto merge without conflicts, take the result.
                        if !merged_index.has_conflicts() {
                            let base_tree = merged_index.write_tree_to(transaction.repo())?;
                            tid = filter::unapply(
                                transaction,
                                filterobj,
                                tree,
                                transaction.repo().find_tree(base_tree)?,
                            )?
                            .id();
                        }
                    }
                }

                if tid == git2::Oid::zero() {
                    // We give up. If we see this message again we need to investigate once
                    // more and maybe consider allowing a manual override as last resort.
                    tracing::warn!("rejecting merge");
                    let msg = format!(
                        "rejecting merge with {} parents:\n{:?}\n1) {:?}\n2) {:?}",
                        parent_count,
                        module_commit.summary().unwrap_or_default(),
                        original_parents_refs[0].summary().unwrap_or_default(),
                        original_parents_refs[1].summary().unwrap_or_default(),
                    );
                    return Err(josh_error(&msg));
                }

                transaction.repo().find_tree(tid)?
            }
        };

        let original_parents_refs_oxide = original_parents_refs
            .iter()
            .map(|parent| parent.id().to_oxide())
            .collect::<Vec<_>>();

        let module_commit_oxide = transaction
            .oxide_repo()
            .find_object(module_commit.id().to_oxide())?
            .into_commit();

        ret = rewrite_commit(
            transaction.oxide_repo(),
            &module_commit_oxide,
            &original_parents_refs_oxide,
            new_tree.id().to_oxide(),
            None,
            None,
            false,
        )?
        .to_git2();

        ret = if original_parents_refs.len() == 1
            && new_tree.id() == original_parents_refs[0].tree_id()
            && Some(module_commit.tree_id()) != module_commit.parents().next().map(|x| x.tree_id())
        {
            original_parents_refs[0].id()
        } else {
            if let Some(ref mut change_ids) = change_ids {
                change_ids.push(get_change_id(&module_commit, ret));
            }
            ret
        };

        bm.insert(module_commit.id(), ret);
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
        transaction.oxide_repo(),
        original_commit,
        filtered_parent_ids,
        filtered_tree,
        None,
        None,
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
    filtered_tree: git2::Tree<'a>,
    transaction: &cache::Transaction,
    filter: filter::Filter,
    author: Option<(String, String)>,
    message: Option<String>,
) -> JoshResult<git2::Oid> {
    let (r, is_new) = create_filtered_commit2(
        transaction.repo(),
        transaction.oxide_repo(),
        original_commit,
        filtered_parent_ids,
        filtered_tree,
        author,
        message,
        false,
    )?;

    let store = is_new || original_commit.parent_ids().len() != 1;

    transaction.insert(filter, original_commit.id(), r, store);

    Ok(r)
}

fn create_filtered_commit2<'a>(
    repo: &'a git2::Repository,
    oxide_repo: &'a gix::Repository,
    original_commit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
    author: Option<(String, String)>,
    message: Option<String>,
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
        filtered_tree.id(),
        filtered_parent_commits.iter().collect(),
    );

    if selected_filtered_parent_commits.is_empty()
        && !(original_commit.parents().len() == 0 && is_empty_root(repo, &original_commit.tree()?))
    {
        if !filtered_parent_commits.is_empty() {
            return Ok((filtered_parent_commits[0].id(), false));
        }
        if filtered_tree.id() == filter::tree::empty_id() {
            return Ok((git2::Oid::zero(), false));
        }
    }

    let original_commit_oxide = oxide_repo
        .find_object(original_commit.id().to_oxide())?
        .into_commit();

    let selected_filtered_parent_commits_oxide = selected_filtered_parent_commits
        .iter()
        .map(|parent| parent.id().to_oxide())
        .collect::<Vec<_>>();

    Ok((
        rewrite_commit(
            oxide_repo,
            &original_commit_oxide,
            &selected_filtered_parent_commits_oxide,
            filtered_tree.id().to_oxide(),
            author,
            message,
            unsign,
        )?
        .to_git2(),
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
