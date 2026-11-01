//! GitHub-side caching of admission conditions.
//!
//! The shared model ([`AdmissionData`], [`evaluate`]) lives in josh-changes;
//! this module keeps the GitHub-specific parts: the TTL for the cached copy
//! and the store/read helpers for the `gh_admission/` namespace of the
//! changes ref.

use josh_changes::AdmissionData;

use josh_changes::ChangesRef;
use josh_core::cache::Transaction;

use crate::layout::{GithubChangesRefData, GITHUB_ADMISSION_PATH};

/// How long a cached `AdmissionData` is reused before refetching: one week.
/// Maintainers and rulesets change rarely, and every fetch is several
/// repository-level GraphQL calls.
pub const ADMISSION_TTL_SECS: u64 = 7 * 24 * 3600;

/// Store the remote's admission data: a sparse `GithubChangesRefData`
/// carrying only the `gh_admission/` subtree, merged into the ref.
pub fn store_admission_data(
    transaction: &Transaction,
    data: &AdmissionData,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let sparse = GithubChangesRefData {
        gh_admission: data.clone(),
        ..Default::default()
    };
    josh_changes::write_filtered(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_ADMISSION_PATH),
        &sparse,
        None,
        None,
    )?;
    Ok(())
}

/// Read the cached admission data, if present. Fails when the stored tree
/// does not decode (`sync --clean` rebuilds the ref).
pub fn read_admission_data(
    transaction: &Transaction,
    scope: &ChangesRef,
) -> anyhow::Result<Option<AdmissionData>> {
    let Some(data) = josh_changes::read_filtered::<GithubChangesRefData>(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_ADMISSION_PATH),
    )?
    else {
        return Ok(None);
    };
    let data = data.gh_admission;
    // An absent subtree decodes to the default struct; fetched_at == 0 is
    // the "never fetched" sentinel.
    Ok((data.fetched_at > 0).then_some(data))
}
