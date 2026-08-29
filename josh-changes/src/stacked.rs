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
            seen.insert(id);
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

            let mut push_refs = changes_to_refs(transaction, baseref, author, changes)?;

            let target = baseref.replacen("refs/heads/", "", 1);
            push_refs.push(PushRef {
                ref_name: StackedRef::StackHead {
                    target: target.clone(),
                    author: author.clone(),
                }
                .ref_name(),
                oid: oid_to_push,
                change_id: target,
            });

            push_refs.sort_by(|a, b| a.ref_name.cmp(&b.ref_name));
            Ok(push_refs)
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
