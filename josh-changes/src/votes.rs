use std::collections::HashMap;

use crate::change::{Change, encode_change_id_path};
use crate::layout::{ChangesRefData, VotesByChange};
use crate::refs::ChangesRef;
use crate::store::{namespace_filter, read_filtered};

use josh_core::cache::Transaction;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoteData {
    pub state: String,
    pub sha: String,
}

/// Which votes namespace a write targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteNamespace {
    /// The canonical `votes` tree.
    Default,
    /// The `outbox/votes` queue of a `Remote` ref: pending posts to the
    /// remote, cleaned up on the next fetch that observes them coming back.
    Outbox,
}

impl VoteNamespace {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Default => "votes",
            Self::Outbox => "outbox/votes",
        }
    }

    /// This namespace's map inside a (possibly sparsely populated)
    /// `ChangesRefData`.
    fn of(self, data: &ChangesRefData) -> &VotesByChange {
        match self {
            Self::Default => &data.votes,
            Self::Outbox => &data.outbox.votes,
        }
    }

    fn of_mut(self, data: &mut ChangesRefData) -> &mut VotesByChange {
        match self {
            Self::Default => &mut data.votes,
            Self::Outbox => &mut data.outbox.votes,
        }
    }

    /// The write namespace for `scope`: local writes go to `Default`, remote
    /// writes queue in `Outbox`.
    pub fn for_scope(scope: &ChangesRef) -> Self {
        match scope {
            ChangesRef::Local { .. } => Self::Default,
            ChangesRef::Remote { .. } => Self::Outbox,
        }
    }
}

/// Write a vote into `namespace` on `scope`'s ref. `Outbox` requires a
fn configured_email(transaction: &Transaction) -> anyhow::Result<String> {
    let signature = transaction.signature()?;
    Ok(std::str::from_utf8(signature.email.as_ref())
        .unwrap_or("unknown")
        .to_owned())
}

/// Write a vote into `namespace` on `scope`'s ref. `Outbox` requires a
/// `Remote` scope. Outbox votes are queued for the next `sync --push` to
/// post to the forge, after which the forge's posted-vote tracking records
/// the post and the outbox entry can be cleaned up.
pub fn write_vote(
    transaction: &Transaction,
    change: &Change,
    state: &str,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
    namespace: VoteNamespace,
) -> anyhow::Result<()> {
    if namespace == VoteNamespace::Outbox && !matches!(scope, ChangesRef::Remote { .. }) {
        return Err(anyhow::anyhow!(
            "outbox votes require a Remote scope (got {})",
            scope.ref_name()
        ));
    }

    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let data = VoteData {
        state: state.to_string(),
        sha: change.commit().to_string(),
    };

    let user = match author {
        Some(name) => name.to_string(),
        None => configured_email(transaction)?,
    };

    let inner: HashMap<String, VoteData> = [(user, data)].into();
    let mut ref_data = ChangesRefData::default();
    namespace
        .of_mut(&mut ref_data)
        .insert(change_id.to_string(), inner);

    crate::store::write_filtered(
        transaction,
        scope,
        namespace_filter(namespace.path()),
        &ref_data,
        author,
        timestamp,
    )?;
    Ok(())
}

pub fn read_vote(
    transaction: &Transaction,
    change_id: &str,
    user: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<Option<VoteData>> {
    let user = match user {
        Some(name) => name.to_string(),
        None => configured_email(transaction)?,
    };

    let Some(data) = read_filtered::<ChangesRefData>(
        transaction,
        scope,
        namespace_filter(VoteNamespace::Default.path()),
    )?
    else {
        return Ok(None);
    };
    Ok(VoteNamespace::Default
        .of(&data)
        .get(change_id)
        .and_then(|votes| votes.get(&user))
        .cloned())
}

pub fn list_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    let data = read_filtered::<ChangesRefData>(
        transaction,
        scope,
        namespace_filter(VoteNamespace::Default.path()),
    )?;
    Ok(sorted_votes(
        data.as_ref()
            .and_then(|d| VoteNamespace::Default.of(d).get(change_id)),
    ))
}

/// List votes queued in the outbox subtree of `scope` (must be Remote in
/// practice; this just returns empty for refs that lack `outbox/votes`).
pub fn list_outbox_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    let data = read_filtered::<ChangesRefData>(
        transaction,
        scope,
        namespace_filter(VoteNamespace::Outbox.path()),
    )?;
    Ok(sorted_votes(
        data.as_ref()
            .and_then(|d| VoteNamespace::Outbox.of(d).get(change_id)),
    ))
}

/// Map entries as a list sorted by user: tree iteration order, which the
/// previous manual walk produced.
fn sorted_votes(votes: Option<&HashMap<String, VoteData>>) -> Vec<(String, VoteData)> {
    let mut votes: Vec<_> = votes
        .map(|m| m.iter().map(|(u, v)| (u.clone(), v.clone())).collect())
        .unwrap_or_default();
    votes.sort_by(|a, b| a.0.cmp(&b.0));
    votes
}

/// Delete the outbox vote entries of the given users from `scope`'s ref.
pub fn delete_outbox_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
    users: &[String],
) -> anyhow::Result<()> {
    let encoded = encode_change_id_path(change_id);
    let paths: Vec<std::path::PathBuf> = users
        .iter()
        .map(|user| {
            std::path::Path::new(VoteNamespace::Outbox.path())
                .join(&encoded)
                .join(user)
        })
        .collect();
    crate::store::delete_filtered(transaction, &paths, scope)
}
