//! State model of the test forge.
//!
//! The test forge is a test-suite fixture: its server-side state (CI check
//! results, reviews, the maintainer set, per-branch admission rules) lives in
//! a standalone ref, [`TEST_FORGE_REF`], in the *remote* repository. The
//! hidden `josh forge` command writes it; `josh changes sync` pulls it like
//! GitHub data.
//!
//! State strings are the serde representations of the shared GitHub types
//! ([`CheckState`], [`PullRequestReviewState`]), so data synced from this
//! tree is interchangeable with what GitHub sync stores.

use std::collections::BTreeMap;

use josh_core::cache::{Expected, Transaction};
use josh_core::objects;
use serde::{Deserialize, Serialize};

pub use josh_github_graphql::operations::get_commit_check_runs::CheckState;
pub use josh_github_webhooks::webhook_types::PullRequestReviewState;

/// Ref in the remote repository holding the test forge's state tree.
pub const TEST_FORGE_REF: &str = "refs/josh/forges/test";

/// Server-side state of the test forge; the whole tree at [`TEST_FORGE_REF`].
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TestForgeState {
    /// commit oid -> check name -> state.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, BTreeMap<String, CheckState>>,
    /// change-id -> user -> state.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reviews: BTreeMap<String, BTreeMap<String, PullRequestReviewState>>,
    /// Unit values: the git-tree format has no sequences, so this is a set of
    /// entries (same shape as `AdmissionData.maintainers`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub maintainers: BTreeMap<String, ()>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub admission: BTreeMap<String, BranchAdmission>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BranchAdmission {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_checks: BTreeMap<String, ()>,
    /// Required approving reviews, as a decimal string. `AdmissionData`
    /// stores its u32 as a little-endian blob, which renders as raw bytes in
    /// tree dumps; this tree is meant to be asserted on in tests, so
    /// readability wins over sharing the binary encoding. "0" (or empty)
    /// means the evaluation floor of one maintainer approval applies.
    #[serde(default)]
    pub required_approvals: String,
}

/// The state tree at [`TEST_FORGE_REF`], or the default when the ref does not
/// exist yet.
pub fn read_state(transaction: &Transaction) -> anyhow::Result<TestForgeState> {
    let Some(tip) = transaction.resolve_ref(TEST_FORGE_REF)? else {
        return Ok(TestForgeState::default());
    };
    let tree = objects::CommitData::read(transaction.odb(), tip)?.tree_id()?;
    let value = josh_git_serde::from_tree_oid(transaction.odb(), tree)?;
    Ok(josh_git_serde::from_value(&value)?)
}

/// Read-modify-write the state tree in a single commit on [`TEST_FORGE_REF`].
/// The whole tree is rewritten (not overlay-merged), so removals in `f` take
/// effect. No-op when `f` leaves the state unchanged.
pub fn update_state(
    transaction: &Transaction,
    message: &str,
    f: impl FnOnce(&mut TestForgeState),
) -> anyhow::Result<()> {
    let mut state = read_state(transaction)?;
    f(&mut state);

    let value = josh_git_serde::to_value(&state)?;
    let tree = josh_git_serde::to_tree_oid(transaction.odb(), &value)?;

    let prev = transaction.resolve_ref(TEST_FORGE_REF)?;
    let prev_tree = prev
        .map(|oid| objects::CommitData::read(transaction.odb(), oid)?.tree_id())
        .transpose()?;
    if prev_tree == Some(tree) {
        return Ok(());
    }

    let sig = josh_core::git::josh_actor_signature()?;
    let parents: Vec<_> = prev.into_iter().collect();
    let commit = objects::write_commit(transaction.odb(), tree, &parents, &sig, &sig, message)?;
    transaction.update_ref(
        TEST_FORGE_REF,
        prev.map_or(Expected::Absent, Expected::At),
        commit,
        message,
    )?;
    Ok(())
}

/// Set the state of check `name` on commit `oid`. The commit must exist in
/// the remote repository: check runs attach to a head commit, so a typo'd oid
/// is almost certainly a mistake.
pub fn set_check(
    transaction: &Transaction,
    oid: gix_hash::ObjectId,
    name: &str,
    state: CheckState,
) -> anyhow::Result<()> {
    objects::CommitData::read(transaction.odb(), oid)
        .map_err(|_| anyhow::anyhow!("commit {oid} not found in the remote repository"))?;
    update_state(transaction, "josh forge check set\n", |s| {
        s.checks
            .entry(oid.to_string())
            .or_default()
            .insert(name.to_string(), state);
    })
}

pub fn set_review(
    transaction: &Transaction,
    change_id: &str,
    user: &str,
    state: PullRequestReviewState,
) -> anyhow::Result<()> {
    update_state(transaction, "josh forge review\n", |s| {
        s.reviews
            .entry(change_id.to_string())
            .or_default()
            .insert(user.to_string(), state);
    })
}

/// Replace `branch`'s admission requirements wholesale: the stored
/// required-checks list becomes exactly `required_checks` (0 approvals =
/// floor of one maintainer approval).
pub fn set_admission(
    transaction: &Transaction,
    branch: &str,
    required_checks: &[String],
    required_approvals: u32,
) -> anyhow::Result<()> {
    update_state(transaction, "josh forge admission set\n", |s| {
        s.admission.insert(
            branch.to_string(),
            BranchAdmission {
                required_checks: required_checks
                    .iter()
                    .map(|check| (check.clone(), ()))
                    .collect(),
                required_approvals: required_approvals.to_string(),
            },
        );
    })
}

pub fn add_maintainer(transaction: &Transaction, user: &str) -> anyhow::Result<()> {
    update_state(transaction, "josh forge maintainer add\n", |s| {
        s.maintainers.insert(user.to_string(), ());
    })
}

pub fn remove_maintainer(transaction: &Transaction, user: &str) -> anyhow::Result<()> {
    update_state(transaction, "josh forge maintainer remove\n", |s| {
        s.maintainers.remove(user);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use josh_core::cache::{CacheStack, SledCacheBackend, TransactionContext};

    fn open_transaction(td: &tempfile::TempDir) -> Transaction {
        gix::init_bare(td.path()).unwrap();
        let cachestack =
            std::sync::Arc::new(CacheStack::new().with_backend(SledCacheBackend::new(td.path())));
        TransactionContext::new(td.path(), cachestack)
            .open()
            .unwrap()
    }

    fn commit(td: &tempfile::TempDir) -> gix_hash::ObjectId {
        let repo = gix::open(td.path()).unwrap();
        let sig = gix_actor::Signature {
            name: "test".into(),
            email: "test@example.com".into(),
            time: gix_actor::date::Time {
                seconds: 0,
                offset: 0,
            },
        };
        josh_gix_ext::write_commit(
            &repo.objects,
            josh_core::filter::tree::empty_id(),
            &[],
            &sig,
            &sig,
            "subject\n",
        )
        .unwrap()
    }

    #[test]
    fn state_roundtrip_and_replacements() {
        let td = tempfile::tempdir().unwrap();
        let t = open_transaction(&td);
        let oid = commit(&td);

        set_check(&t, oid, "build", CheckState::Pending).unwrap();
        set_check(&t, oid, "build", CheckState::Success).unwrap();
        set_review(&t, "change/1", "alice", PullRequestReviewState::Approved).unwrap();
        add_maintainer(&t, "alice").unwrap();
        add_maintainer(&t, "bob").unwrap();
        remove_maintainer(&t, "bob").unwrap();
        set_admission(&t, "master", &["build".to_string()], 2).unwrap();

        let state = read_state(&t).unwrap();
        assert_eq!(state.checks[&oid.to_string()]["build"], CheckState::Success);
        assert_eq!(
            state.reviews["change/1"]["alice"],
            PullRequestReviewState::Approved
        );
        assert!(state.maintainers.contains_key("alice"));
        assert!(!state.maintainers.contains_key("bob"));
        let admission = &state.admission["master"];
        assert!(admission.required_checks.contains_key("build"));
        assert_eq!(admission.required_approvals, "2");

        set_admission(&t, "master", &["lint".to_string()], 0).unwrap();
        let state = read_state(&t).unwrap();
        let admission = &state.admission["master"];
        assert!(admission.required_checks.contains_key("lint"));
        assert!(!admission.required_checks.contains_key("build"));
        assert_eq!(admission.required_approvals, "0");

        let missing =
            gix_hash::ObjectId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        assert!(set_check(&t, missing, "build", CheckState::Success).is_err());
    }
}
