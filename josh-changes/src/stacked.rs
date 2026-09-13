//! Stacked-changes push machinery: deciding which refs a push must create or
//! update.

use crate::change::{Change, get_changes, split_changes};
use crate::refs::{StackedChangeRef, StackedRef};
use anyhow::anyhow;
use josh_core::cache::Transaction;

#[derive(PartialEq, Clone, Debug)]
pub enum PushMode {
    Normal,
    Publish(String),
}

#[derive(Debug, Clone)]
pub struct PushRef {
    pub ref_name: String,
    pub oid: gix_hash::ObjectId,
    pub change_id: String,
}

pub(crate) fn changes_to_refs(
    transaction: &Transaction,
    baseref: &str,
    change_author: &str,
    changes: Vec<Change>,
) -> anyhow::Result<Vec<PushRef>> {
    if !change_author.contains('@') {
        return Err(anyhow!(
            "Push option 'author' needs to be set to a valid email address",
        ));
    };

    let changes: Vec<Change> = changes
        .into_iter()
        .filter(|change| change.author == change_author)
        .collect();

    let mut seen = std::collections::HashSet::new();
    for change in changes.iter() {
        if let Some(id) = &change.id {
            if id.contains('@') {
                return Err(anyhow!("Change id must not contain '@'"));
            }
            if !seen.insert(id) {
                return Err(anyhow!(
                    "rejecting to push {:?} with duplicate label",
                    change.commit
                ));
            }
        }
    }

    let mut refs = vec![];
    for change in changes {
        if let Some(change_id) = change.id {
            let change_ref = StackedChangeRef::Change {
                target: baseref.replacen("refs/heads/", "", 1),
                author: change.author,
                change_id: change_id.clone(),
            };
            refs.push(PushRef {
                ref_name: StackedRef::ChangeRef(change_ref.clone()).ref_name(),
                oid: change.commit,
                change_id: change_id.clone(),
            });
            if let Some(parent_sha) =
                josh_core::objects::CommitData::read(transaction.odb(), change.commit)?
                    .first_parent_id()
            {
                refs.push(PushRef {
                    ref_name: StackedRef::ChangeRef(change_ref.as_base()).ref_name(),
                    oid: parent_sha,
                    change_id,
                });
            }
        }
    }
    Ok(refs)
}

/// The shared tail of publish ref building: per-change `@changes`/`@base`
/// refs plus the `@heads` stack tip, sorted by ref name. `target` may be
/// fully qualified (`refs/heads/master`); the `refs/heads/` prefix is
/// stripped here, the single normalization point.
fn publish_refs(
    transaction: &Transaction,
    target: &str,
    author: &str,
    changes: Vec<Change>,
    head_oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<PushRef>> {
    let target = target.replacen("refs/heads/", "", 1);

    let mut push_refs = changes_to_refs(transaction, &target, author, changes)?;

    push_refs.push(PushRef {
        ref_name: StackedRef::StackHead {
            target: target.clone(),
            author: author.to_string(),
        }
        .ref_name(),
        oid: head_oid,
        change_id: target,
    });

    push_refs.sort_by(|a, b| a.ref_name.cmp(&b.ref_name));
    Ok(push_refs)
}

/// Build the refs for publishing to a link: the same change refs as
/// `build_to_push`'s `Publish` mode, projected through the link's filter.
/// Changes that become empty under the filter are dropped. Returns an empty
/// vec when nothing survives; the caller then skips the link entirely,
/// including the `@heads` ref.
pub fn build_link_push(
    transaction: &Transaction,
    author: &str,
    target: &str,
    oid_to_push: gix_hash::ObjectId,
    base_oid: gix_hash::ObjectId,
    link_filter: josh_core::filter::Filter,
) -> anyhow::Result<Vec<PushRef>> {
    let changes = get_changes(transaction, oid_to_push, base_oid)?;
    let changes = split_changes(transaction, changes)?;

    // A change is empty under the link's filter when its filtered tree equals
    // its filtered parent's tree. Note filtering collapses an empty commit
    // onto its parent, so the mapped commit's own parent is not reliable —
    // filter the original first parent explicitly.
    let odb = transaction.odb();
    let mut filtered_changes = Vec::new();
    for change in changes {
        let commit = josh_core::filter::apply_to_commit(link_filter, change.commit, transaction)?;
        let tree = josh_core::objects::CommitData::read(odb, commit)?.tree_id()?;
        let parent_tree = josh_core::objects::CommitData::read(odb, change.commit)?
            .first_parent_id()
            .map(|parent| {
                let filtered =
                    josh_core::filter::apply_to_commit(link_filter, parent, transaction)?;
                josh_core::objects::CommitData::read(odb, filtered)?.tree_id()
            })
            .transpose()?;
        if parent_tree == Some(tree) {
            continue;
        }
        filtered_changes.push(Change { commit, ..change });
    }

    if filtered_changes.is_empty() {
        return Ok(vec![]);
    }

    let head_oid = josh_core::filter::apply_to_commit(link_filter, oid_to_push, transaction)?;
    publish_refs(transaction, target, author, filtered_changes, head_oid)
}

pub fn build_to_push(
    transaction: &Transaction,
    push_mode: &PushMode,
    baseref: &str,
    ref_with_options: &str,
    oid_to_push: gix_hash::ObjectId,
    base_oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<PushRef>> {
    match push_mode {
        PushMode::Publish(author) => {
            let changes = get_changes(transaction, oid_to_push, base_oid)?;
            let changes = split_changes(transaction, changes)?;

            publish_refs(transaction, baseref, author, changes, oid_to_push)
        }
        PushMode::Normal => Ok(vec![PushRef {
            ref_name: if ref_with_options.starts_with("refs/") {
                ref_with_options.to_string()
            } else {
                format!("refs/heads/{}", ref_with_options)
            },
            oid: oid_to_push,
            change_id: "JOSH_PUSH".to_string(),
        }]),
    }
}
