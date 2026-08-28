use super::*;
use anyhow::anyhow;
use std::collections::{BTreeMap, HashMap, HashSet};

pub enum GpgsigMode {
    Preserve,
    Remove,
    NormLf,
}

/// Returns true if a comma-separated meta value contains `flag` (trimmed).
/// The `history` meta accepts multiple flags, e.g. `history="linear,no-splice"`.
pub(crate) fn history_flag(value: Option<&str>, flag: &str) -> bool {
    value.is_some_and(|v| v.split(',').any(|f| f.trim() == flag))
}

pub fn walk2(
    filter: filter::Filter,
    input: gix_hash::ObjectId,
    transaction: &cache::Transaction,
) -> anyhow::Result<()> {
    if transaction.known(filter, input)? {
        return Ok(());
    }

    let odb = transaction.odb();

    // `walk.push` rejects a missing or non-commit input as a hard error.
    let mut walk = objects::RevWalk::new(odb);
    if history_flag(filter.get_meta("history").as_deref(), "linear") {
        walk.simplify_first_parent();
    }
    walk.push(input)?;

    log::info!(
        "Walking {} commits for: {} {:?}",
        crate::cache::compute_sequence_number(transaction, input)?,
        filter::spec(filter),
        input,
    );

    // The prune callback cannot propagate errors, so treat a failed lookup as "not known":
    // the walk then visits the commit and the fallible body reports the same error properly.
    let sorted = walk.into_topo_vec(|id| transaction.known(filter, id).unwrap_or(false))?;

    let mut n_in = 0;
    let mut n_out = 0;

    for &id in sorted.iter().rev() {
        if filter::apply_to_commit2(filter, id, transaction)?.is_some() {
            n_out += 1;
        } else {
            break;
        }

        n_in += 1;
        if n_in % 1000 == 0 {
            log::debug!("{} commits filtered, {} written", n_in, n_out,);
        }
    }

    log::info!("{} commits filtered, {} written", n_in, n_out,);

    Ok(())
}

fn find_unapply_base(
    transaction: &cache::Transaction,
    // Used as a cache to avoid re-applying the filter to the same commit -
    // this function is called during revwalk so there be a lot of repeated
    // calls
    filtered_to_original: &mut HashMap<gix_hash::ObjectId, gix_hash::ObjectId>,
    filter: filter::Filter,
    // When building the filtered_to_original mapping use this as a starting point
    // for the search for originals. If there are multiple originals that map to the
    // same filtered commit (which is common) use one that is reachable from contained_in.
    // Or, in other words, one that is contained in the history of contained_in.
    contained_in: gix_hash::ObjectId,
    // Filtered OID to compare against
    filtered: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    // Consult the running map first: during an unapply walk we insert every
    // freshly created commit here, so a later commit can find its parent even
    // when there is no `contained_in` hint (e.g. a no-base push of an orphan
    // history). Checking this before the zero guard is what keeps such a push
    // connected instead of collapsing into a single parentless commit.
    if let Some(original) = filtered_to_original.get(&filtered) {
        tracing::info!("Found in filtered_to_original",);
        return Ok(*original);
    }

    if contained_in == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
        tracing::info!("contained in zero",);
        return Ok(gix_hash::ObjectId::null(gix_hash::Kind::Sha1));
    }

    let oid = filter::apply_to_commit(filter, contained_in, transaction)?;
    if oid != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
        filtered_to_original.insert(oid, contained_in);
    }
    if filtered == oid {
        return Ok(contained_in);
    }

    // Search newest-generation first so recent side branches stay ahead of older first-parent
    // chains. Sequence numbers are persisted with the filtering cache, so the search stays lazy.
    let odb = transaction.odb();
    let sequence_number = |oid| cache::compute_sequence_number(transaction, oid);
    let mut frontier = objects::GenerationFrontier::new();
    let mut seen = HashSet::new();
    frontier.push(sequence_number(contained_in)?, contained_in);

    while let Some(candidate) = frontier.pop() {
        if !seen.insert(candidate) {
            continue;
        }
        let original_filtered = filter::apply_to_commit(filter, candidate, transaction)?;
        if filtered == original_filtered {
            filtered_to_original.insert(filtered, candidate);
            tracing::info!("found original properly {}", candidate);
            return Ok(candidate);
        }

        for parent_id in git::read_parent_ids(odb, candidate)? {
            if seen.contains(&parent_id) {
                continue;
            }
            frontier.push(sequence_number(parent_id)?, parent_id);
        }
    }

    tracing::info!("Didn't find original",);
    Ok(gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
}

pub fn find_original(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    contained_in: gix_hash::ObjectId,
    filtered: gix_hash::ObjectId,
    linear: bool,
) -> anyhow::Result<gix_hash::ObjectId> {
    if contained_in == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
        return Ok(gix_hash::ObjectId::null(gix_hash::Kind::Sha1));
    }
    if filter.is_nop() {
        return Ok(filtered);
    }
    let odb = transaction.odb();
    let mut walk = objects::RevWalk::new(odb);
    if linear {
        walk.simplify_first_parent();
    }
    walk.push(contained_in)?;

    for original in walk.into_topo_vec(|_| false)? {
        if filtered == filter::apply_to_commit(filter, original, transaction)? {
            let parent_ids = git::read_parent_ids(odb, original)?;
            if parent_ids.len() == 1 {
                let fp = filter::apply_to_commit(filter, parent_ids[0], transaction)?;

                if fp == filtered {
                    continue;
                }
            }
            return Ok(original);
        }
    }

    Ok(gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
}

// takes everything from base except its tree and replaces it with the tree
// given
pub fn rewrite_commit(
    odb: &josh_memodb::Odb,
    base: &objects::CommitData,
    parents: &[gix_hash::ObjectId],
    rewrite_data: filter::Rewrite,
    gpgsig: GpgsigMode,
) -> anyhow::Result<gix_hash::ObjectId> {
    use gix_object::bstr::BString;

    // gix_object::CommitRef uses byte strings for Oids, but in hex representation, not raw bytes.
    // Its `Format` implementation writes out hex-encoded bytes. Because of CommitRef's reference
    // lifetimes we have to this, before creating CommitRef
    let tree_id = BString::from(rewrite_data.tree_id().to_string());
    let parent_ids = parents
        .iter()
        .map(|p| BString::from(p.to_string()))
        .collect::<Vec<_>>();

    let mut author = None;
    let mut committer = None;
    let message = rewrite_data.message;

    let mut commit = gix_object::CommitRef::from_bytes(base.bytes(), gix_hash::Kind::Sha1)?;
    commit.tree = tree_id.as_ref();

    commit.parents.clear();
    commit
        .parents
        .extend(parent_ids.iter().map(BString::as_ref));

    let rewrite_signature =
        |name: BString, email: BString, time: &str| -> anyhow::Result<BString> {
            let signature = gix_actor::SignatureRef {
                name: name.as_ref(),
                email: email.as_ref(),
                time,
            };

            let mut buffer = Vec::new();
            signature.write_to(&mut buffer)?;

            Ok(BString::from(buffer))
        };

    // NameEmail re-serializes with the base commit's own timestamp; Raw is a verbatim
    // byte transplant of a complete signature field, nothing parsed.
    if let Some(sig) = rewrite_data.author {
        author = Some(match sig {
            filter::SigRewrite::NameEmail(name, email) => {
                rewrite_signature(name, email, commit.author()?.time)?
            }
            filter::SigRewrite::Raw(raw) => raw,
        });
    }

    if let Some(sig) = rewrite_data.committer {
        committer = Some(match sig {
            filter::SigRewrite::NameEmail(name, email) => {
                rewrite_signature(name, email, commit.committer()?.time)?
            }
            filter::SigRewrite::Raw(raw) => raw,
        });
    }

    if let Some(author) = &author {
        commit.author = author.as_ref();
    }

    if let Some(committer) = &committer {
        commit.committer = committer.as_ref();
    }

    if let Some(message) = &message {
        commit.message = message.as_ref();
    }

    match gpgsig {
        GpgsigMode::Remove => {
            commit
                .extra_headers
                .retain(|(k, _)| *k != "gpgsig".as_bytes());
        }
        GpgsigMode::NormLf => {
            use gix_object::bstr::ByteSlice;
            for (k, v) in commit.extra_headers.iter_mut() {
                if *k == "gpgsig".as_bytes() && v.contains_str(b"\r\n") {
                    *v = std::borrow::Cow::Owned(v.replace(b"\r\n", b"\n").into());
                }
            }
        }
        GpgsigMode::Preserve => {}
    }

    let mut b = vec![];
    gix_object::WriteTo::write_to(&commit, &mut b)?;

    Ok(odb.write(gix_object::Kind::Commit, &b))
}

// Given an OID of an unfiltered commit and a filter,
// find the oldest commit (within the topological order)
// that gives the same result when filtered
fn find_oldest_similar_commit(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    unfiltered: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    let mut walk = objects::RevWalk::new(odb);
    walk.push(unfiltered)?;
    tracing::info!("oldest similar?");
    let filtered = filter::apply_to_commit(filter, unfiltered, transaction)?;
    let mut prev_rev = unfiltered;
    for rev in walk.into_topo_vec(|_| false)? {
        tracing::info!("next");
        if filtered != filter::apply_to_commit(filter, rev, transaction)? {
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
    filtered_to_original: &mut HashMap<gix_hash::ObjectId, gix_hash::ObjectId>,
    filter: filter::Filter,
    // See "contained_in" in find_unapply_base
    contained_in: gix_hash::ObjectId,
    filtered: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    let mut walk = objects::RevWalk::new(odb);
    walk.push(filtered)?;
    tracing::info!("new branch base?");

    // Walk filtered history, trying to find a base for every commit
    for rev in walk.into_topo_vec(|_| false)? {
        if let Ok(base) =
            find_unapply_base(transaction, filtered_to_original, filter, contained_in, rev)
            && base != gix_hash::ObjectId::null(gix_hash::Kind::Sha1)
        {
            tracing::info!("new branch base: {:?} mapping to {:?}", base, rev);
            let base = if let Ok(new_base) = find_oldest_similar_commit(transaction, filter, base) {
                new_base
            } else {
                base
            };

            tracing::info!("inserting in filtered_to_original {}, {}", rev, base);
            filtered_to_original.insert(rev, base);

            return Ok(rev);
        }
    }
    tracing::info!("new branch base not found");
    Ok(gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
}

#[derive(Clone, Debug)]
pub enum OrphansMode {
    Keep,
    Remove,
    Fail,
}

#[tracing::instrument(skip(transaction))]
pub fn unapply_filter(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    original_target: gix_hash::ObjectId,
    old_filtered_oid: gix_hash::ObjectId,
    new_filtered_oid: gix_hash::ObjectId,
    orphans_mode: OrphansMode,
    reparent_orphans: Option<gix_hash::ObjectId>,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut filtered_to_original = HashMap::new();
    let mut ret = original_target;

    let old_filtered_oid = if old_filtered_oid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
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

    let odb = transaction.odb();

    // The old filtered oid can be missing from the repo (e.g. a new branch);
    // there is no range to exclude then, so take everything reachable.
    let old_filtered_exists = matches!(
        odb.read_header(old_filtered_oid),
        Ok((gix_object::Kind::Commit, _))
    );
    let revs = if old_filtered_exists {
        // The sequence lookup computes hints on demand and is only consulted
        // when the fast-forward path cannot answer, so linear pushes never
        // touch the hint cache; computed hints persist, making later
        // non-linear walks incremental. The hint walk is itself a plain
        // pruned RevWalk, so the nested walker cannot recurse further.
        let walk =
            objects::RangeWalk::new(odb, |oid| cache::compute_sequence_number(transaction, oid));
        walk.into_topo_vec(new_filtered_oid, old_filtered_oid)?
    } else {
        tracing::warn!("range walk: old filtered oid not found");
        let mut walk = objects::RevWalk::new(odb);
        walk.push(new_filtered_oid)?;
        walk.into_topo_vec(|_| false)?
    };

    // Walk starting from new filtered OID, parents before children
    for &rev in revs.iter().rev() {
        let span = tracing::span!(tracing::Level::TRACE, "walk commit", ?rev);
        let _span_guard = span.enter();

        tracing::info!("walk commit: {:?}", rev);
        let module_commit = objects::CommitData::read(odb, rev)?;

        if filtered_to_original.contains_key(&module_commit.id()) {
            continue;
        }

        let mut filtered_parent_ids: Vec<_> = module_commit.parent_ids().collect();
        let has_new_orphan = filtered_parent_ids.len() > 1
            && objects::merge_base_octopus(odb, &filtered_parent_ids)?.is_none();

        if has_new_orphan {
            match orphans_mode {
                OrphansMode::Keep => {}
                OrphansMode::Remove => {
                    filtered_parent_ids.pop();
                }
                OrphansMode::Fail => {
                    return Err(anyhow!(indoc::formatdoc!(
                        r###"
                        Rejecting new orphan branch at {:?} ({})
                        Specify one of these options:
                          '-o allow_orphans' to keep the history as is
                          '-o merge' to import new history by creating merge commit
                          '-o edit' if you are editing a stored filter or workspace
                        "###,
                        module_commit.summary().unwrap_or_default(),
                        module_commit.id(),
                    )));
                }
            }
        }

        // For every parent of a filtered commit, find unapply base
        let original_parents: Result<Vec<_>, _> = filtered_parent_ids
            .iter()
            .map(|filtered_parent_id| -> anyhow::Result<_> {
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
                    *oid != gix_hash::ObjectId::null(gix_hash::Kind::Sha1)
                } else {
                    true
                }
            })
            .map(|unapply_base| -> anyhow::Result<_> {
                objects::CommitData::read(odb, unapply_base?)
            })
            .collect();

        // If there are no parents and "reparent" option is given, use the given OID as a parent
        let mut original_parents = original_parents?;
        if let (0, Some(reparent)) = (original_parents.len(), reparent_orphans) {
            original_parents = vec![objects::CommitData::read(odb, reparent)?];
        }

        tracing::info!(
            "parents: {:?} -> {:?}",
            original_parents,
            filtered_parent_ids
        );

        let tree = module_commit.tree_id()?;
        let commit_message = module_commit
            .summary()
            .unwrap_or_else(|| "NO COMMIT MESSAGE".to_string());

        let new_trees: anyhow::Result<Vec<_>> = {
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
                .map(|commit| -> anyhow::Result<_> {
                    // Pass the commit context so a `:rev(...)` cutoff in the filter is resolved
                    // per commit (current vs this parent) rather than collapsed uniformly.
                    filter::unapply(
                        transaction,
                        filter,
                        tree,
                        commit.tree_id()?,
                        Some((module_commit.id(), commit.id())),
                    )
                })
                .collect()
        };

        let new_trees = match new_trees {
            Ok(new_trees) => new_trees,
            Err(e) => {
                return Err(anyhow!(
                    "\nCan't apply {:?} ({})\n{}",
                    commit_message,
                    module_commit.id(),
                    e
                ));
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
            1 => new_trees[0],

            // 0 means the history is unrelated. Pushing it will fail if we are not
            // dealing with either a force push or a push with the "merge" option set.
            0 => {
                tracing::debug!("unrelated history");
                // Unrelated history has no original parent; there is no `<=SHA` baseline, so a
                // `:rev(...)` cutoff resolves against `module_commit` on both sides.
                filter::unapply(
                    transaction,
                    filter,
                    tree,
                    filter::tree::empty_id(),
                    Some((module_commit.id(), module_commit.id())),
                )?
            }

            // This will typically be parent_count == 2 and mean we are dealing with a merge
            // where the parents have differences outside of the filter.
            parent_count => {
                let mut tid = gix_hash::ObjectId::null(gix_hash::Kind::Sha1);
                for i in 0..parent_count {
                    // If one of the parents is a descendant of the target branch and the other is
                    // not, pick the tree of the one that is a descendant.
                    if (original_parents[i].id() == original_target)
                        || objects::is_descendant_of(
                            odb,
                            original_parents[i].id(),
                            original_target,
                        )?
                    {
                        tid = new_trees[i];
                        break;
                    }
                }

                if tid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) && parent_count == 2 {
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

                    let base_tree = objects::merge_commits(
                        odb,
                        original_parents[0].id(),
                        original_parents[1].id(),
                        Some(objects::merge::Favor::Ours),
                    )?;
                    // The base is a merge of both original parents; resolve any `:rev(...)` cutoff
                    // against the first parent (mirrors the descendant-pick preference above).
                    let tid_ours = filter::unapply(
                        transaction,
                        filter,
                        tree,
                        base_tree,
                        Some((module_commit.id(), original_parents[0].id())),
                    )?;

                    let base_tree = objects::merge_commits(
                        odb,
                        original_parents[0].id(),
                        original_parents[1].id(),
                        Some(objects::merge::Favor::Theirs),
                    )?;
                    let tid_theirs = filter::unapply(
                        transaction,
                        filter,
                        tree,
                        base_tree,
                        Some((module_commit.id(), original_parents[0].id())),
                    )?;

                    if tid_ours == tid_theirs {
                        tid = tid_ours;
                    }
                }

                if tid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    // We give up. If we see this message again we need to investigate once
                    // more and maybe consider allowing a manual override as last resort.
                    tracing::warn!("rejecting merge");
                    return Err(anyhow!(
                        "rejecting merge with {} parents:\n{:?} ({:?})\n1) {:?} ({:?})\n2) {:?} ({:?})",
                        parent_count,
                        module_commit.summary().unwrap_or_default(),
                        module_commit.id(),
                        original_parents[0].summary().unwrap_or_default(),
                        original_parents[0].id(),
                        original_parents[1].summary().unwrap_or_default(),
                        original_parents[1].id(),
                    ));
                }

                tid
            }
        };

        let apply = filter::Rewrite::from_tree(new_tree);

        let original_parent_oids: Vec<gix_hash::ObjectId> =
            original_parents.iter().map(|c| c.id()).collect();
        ret = rewrite_commit(
            odb,
            &module_commit,
            &original_parent_oids,
            apply,
            GpgsigMode::Preserve,
        )?;

        ret = if original_parents.len() == 1
            && new_tree == original_parents[0].tree_id()?
            && Some(module_commit.tree_id()?)
                != module_commit
                    .first_parent_id()
                    .and_then(|p| git::read_tree_id(odb, p).ok())
        {
            original_parents[0].id()
        } else {
            ret
        };

        filtered_to_original.insert(module_commit.id(), ret);
    }

    tracing::trace!("done {:?}", ret);
    Ok(ret)
}

fn select_parent_commits(
    odb: &josh_memodb::Odb,
    original_commit: &objects::CommitData,
    filtered_tree_id: gix_hash::ObjectId,
    filtered_parents: &[(gix_hash::ObjectId, gix_hash::ObjectId)],
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let affects_filtered = filtered_parents
        .iter()
        .any(|(_, tree_id)| filtered_tree_id != *tree_id);

    let original_tree_id = original_commit.tree_id()?;
    let mut all_diffs_empty = true;
    for p in original_commit.parent_ids() {
        if git::read_tree_id(odb, p)? != original_tree_id {
            all_diffs_empty = false;
            break;
        }
    }

    Ok(if affects_filtered || all_diffs_empty {
        filtered_parents.iter().map(|(id, _)| *id).collect()
    } else {
        vec![]
    })
}

// parents={none, linear, keep-trivial, default}

pub fn drop_commit(
    original_commit: gix_hash::ObjectId,
    filtered_parent_ids: Vec<gix_hash::ObjectId>,
    transaction: &cache::Transaction,
    filter: filter::Filter,
) -> anyhow::Result<gix_hash::ObjectId> {
    let r = if let Some(id) = filtered_parent_ids.first() {
        *id
    } else {
        gix_hash::ObjectId::null(gix_hash::Kind::Sha1)
    };

    transaction.insert(filter, original_commit, r, false)?;

    Ok(r)
}

pub fn create_filtered_commit_with_meta(
    original_commit: &objects::CommitData,
    filtered_parent_ids: Vec<gix_hash::ObjectId>,
    rewrite_data: filter::Rewrite,
    transaction: &cache::Transaction,
    filter: filter::Filter,
    meta: std::collections::BTreeMap<String, String>,
) -> anyhow::Result<gix_hash::ObjectId> {
    let (r, is_new) = create_filtered_commit2(
        transaction,
        original_commit,
        filtered_parent_ids,
        rewrite_data,
        meta,
    )?;

    let store = is_new || original_commit.parent_count() != 1;

    transaction.insert(filter, original_commit.id(), r, store)?;

    Ok(r)
}

pub fn create_filtered_commit(
    original_commit: &objects::CommitData,
    filtered_parent_ids: Vec<gix_hash::ObjectId>,
    rewrite_data: filter::Rewrite,
    transaction: &cache::Transaction,
    filter: filter::Filter,
) -> anyhow::Result<gix_hash::ObjectId> {
    create_filtered_commit_with_meta(
        original_commit,
        filtered_parent_ids,
        rewrite_data,
        transaction,
        filter,
        filter.into_meta(),
    )
}

fn create_filtered_commit2(
    transaction: &cache::Transaction,
    original_commit: &objects::CommitData,
    filtered_parent_ids: Vec<gix_hash::ObjectId>,
    rewrite_data: filter::Rewrite,
    options: BTreeMap<String, String>,
) -> anyhow::Result<(gix_hash::ObjectId, bool)> {
    let odb = transaction.odb();
    let mut filtered_parents: Vec<(gix_hash::ObjectId, gix_hash::ObjectId)> = filtered_parent_ids
        .iter()
        .filter(|x| **x != gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
        .map(|x| Ok((*x, filtered_parent_tree_id(transaction, *x)?)))
        .collect::<anyhow::Result<_>>()?;

    if filtered_parents
        .iter()
        .any(|(_, tree_id)| *tree_id == filter::tree::empty_id())
    {
        // An "initial merge" is a merge whose parents have no common ancestor.
        // Cheaper than `repo.merge_base_many(...).is_err()`: ask whether the
        // parents' reachable-root sets intersect, which is cached per-commit.
        // Elided (zero) parents don't make a commit a merge, so only the
        // surviving parents count here.
        let nonzero_parent_ids: Vec<_> = filtered_parent_ids
            .iter()
            .copied()
            .filter(|x| *x != gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
            .collect();
        let is_initial_merge = nonzero_parent_ids.len() > 1
            && !cache::parents_share_root(transaction, &nonzero_parent_ids)?;

        // Pruning must not leave the commit parentless: a sole surviving
        // empty-tree parent may carry real history behind it.
        if is_initial_merge
            && filtered_parents
                .iter()
                .any(|(_, tree_id)| *tree_id != filter::tree::empty_id())
        {
            filtered_parents.retain(|(_, tree_id)| *tree_id != filter::tree::empty_id());
        }
    }

    if history_flag(options.get("history").map(String::as_str), "linear") {
        filtered_parents.truncate(1);
    }

    if !history_flag(
        options.get("history").map(String::as_str),
        "keep-trivial-merges",
    ) {
        if filtered_parents.len() > 1 {
            let is_trivial_merge = filtered_parents[0].1 == rewrite_data.tree_id();
            // No first parent is not a trivial merge; a present first parent
            // must have a readable tree.
            let was_trivial_merge = match original_commit.first_parent_id() {
                Some(parent) => git::read_tree_id(odb, parent)? == original_commit.tree_id()?,
                None => false,
            };

            if is_trivial_merge && !was_trivial_merge {
                // Returning the parent id here means the commit is dropped from the output history
                return Ok((filtered_parents[0].0, false));
            }
        }
    }

    let selected_filtered_parent_ids: Vec<gix_hash::ObjectId> = select_parent_commits(
        odb,
        original_commit,
        rewrite_data.tree_id(),
        &filtered_parents,
    )?;

    if selected_filtered_parent_ids.is_empty()
        && !(original_commit.parent_count() == 0
            && is_empty_root(transaction, odb, original_commit.tree_id()?)?)
    {
        if !filtered_parents.is_empty() {
            // Returning the parent id here means the commit is dropped from the output history
            return Ok((filtered_parents[0].0, false));
        }
        if rewrite_data.tree_id() == filter::tree::empty_id() {
            return Ok((gix_hash::ObjectId::null(gix_hash::Kind::Sha1), false));
        }
    }

    let gpgsig = match options.get("gpgsig").map(String::as_str) {
        Some("remove") => GpgsigMode::Remove,
        Some("norm-lf") => GpgsigMode::NormLf,
        _ => GpgsigMode::Preserve,
    };

    let new_tree_id = rewrite_data.tree_id();
    let new_oid = rewrite_commit(
        odb,
        original_commit,
        &selected_filtered_parent_ids,
        rewrite_data,
        gpgsig,
    )?;
    // Record the freshly written commit's tree so the next commit in this walk (usually its child)
    // reads its parent's tree from the slot instead of re-parsing the commit from the odb.
    transaction.set_last_written_commit(new_oid, new_tree_id);
    Ok((new_oid, true))
}

/// Tree id of a filtered parent. A history walk writes a commit immediately before processing its
/// child, so the parent is almost always the last commit josh wrote and its tree is served from the
/// transaction's single-slot cache; anything else (merge parents, non-linear order) falls back to
/// parsing the commit from the odb.
pub(crate) fn filtered_parent_tree_id(
    transaction: &cache::Transaction,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    if let Some((last, tree_id)) = transaction.last_written_commit() {
        if last == oid {
            return Ok(tree_id);
        }
    }
    git::read_tree_id(transaction.odb(), oid)
}

/// Whether `oid` is a tree containing nothing but (recursively) empty trees. An unreadable
/// root is a hard error, while unreadable or non-tree entries below fold to `false`.
fn is_empty_root(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<bool> {
    if oid == filter::tree::empty_id() {
        return Ok(true);
    }

    let bytes = transaction
        .read_tree_bytes(odb, oid)?
        .ok_or_else(|| anyhow!("{} is not a tree", oid))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes, gix_hash::Kind::Sha1)?;
    Ok(tree
        .entries
        .iter()
        .all(|e| e.mode.is_tree() && is_empty_subtree(transaction, odb, e.oid.to_owned())))
}

fn is_empty_subtree(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    oid: gix_hash::ObjectId,
) -> bool {
    if oid == filter::tree::empty_id() {
        return true;
    }
    let Ok(Some(bytes)) = transaction.read_tree_bytes(odb, oid) else {
        return false;
    };
    let Ok(tree) = gix_object::TreeRef::from_bytes(&bytes, gix_hash::Kind::Sha1) else {
        return false;
    };
    tree.entries
        .iter()
        .all(|e| e.mode.is_tree() && is_empty_subtree(transaction, odb, e.oid.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // A root is "empty" iff it contains nothing but (recursively) empty trees: the empty tree
    // itself and nested empty chains qualify; any blob or gitlink anywhere disqualifies.
    #[test]
    fn is_empty_root_contract() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let cachestack = std::sync::Arc::new(
            cache::CacheStack::new().with_backend(cache::SledCacheBackend::new(td.path())),
        );
        let ctx = cache::TransactionContext::new(td.path(), cachestack);
        let t = ctx.open().unwrap();
        let odb = t.odb();

        let empty = filter::tree::empty_id();
        assert!(is_empty_root(&t, odb, empty).unwrap());

        // A chain of trees bottoming out in the (virtualized) empty tree is empty. Built by
        // hand: normal tree builders drop empty subtrees.
        let mut data = Vec::new();
        data.extend_from_slice(b"40000 sub");
        data.push(0);
        data.extend_from_slice(empty.as_bytes());
        let chain =
            gix_object::Write::write_buf(&repo.objects, gix_object::Kind::Tree, &data).unwrap();
        let mut data = Vec::new();
        data.extend_from_slice(b"40000 nested");
        data.push(0);
        data.extend_from_slice(chain.as_bytes());
        let chain2 =
            gix_object::Write::write_buf(&repo.objects, gix_object::Kind::Tree, &data).unwrap();
        assert!(is_empty_root(&t, odb, chain2).unwrap());

        // A blob anywhere makes the root non-empty; so does a gitlink.
        let blob = josh_gix_ext::write_blob(&repo.objects, b"x").unwrap();
        let mut builder = repo.edit_tree(empty).unwrap();
        builder
            .upsert("a/b/file.txt", gix::objs::tree::EntryKind::Blob, blob)
            .unwrap();
        let with_blob = builder.write().unwrap().detach();
        assert!(!is_empty_root(&t, odb, with_blob).unwrap());

        let gitlink =
            gix_hash::ObjectId::from_str("0123456789012345678901234567890123456789").unwrap();
        let mut builder = repo.edit_tree(empty).unwrap();
        builder
            .upsert("sub", gix::objs::tree::EntryKind::Commit, gitlink)
            .unwrap();
        let with_gitlink = builder.write().unwrap().detach();
        assert!(!is_empty_root(&t, odb, with_gitlink).unwrap());
    }
}
