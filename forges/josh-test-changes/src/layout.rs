//! Serde view of the test-forge-owned namespaces on a changes ref.
//!
//! Mirrors the GitHub layout: where GitHub sync stores into the `gh*`
//! namespaces of `refs/josh/remotes/<remote>/changes/<branch>`, test sync
//! stores into `test*` namespaces of the same ref. The stored value types
//! are the forge-neutral ones from josh-changes (`ChangeData`,
//! `AdmissionData`), so the read path can evaluate admission identically
//! for both forges.

use std::collections::HashMap;

use josh_changes::ChangeData;
use serde::{Deserialize, Serialize};

/// Path of the stored per-change data namespace (`test/<change-id>`).
pub const TEST_CHANGE_DATA_PATH: &str = "test";

/// Path of the admission-data namespace.
pub const TEST_ADMISSION_PATH: &str = "test_admission";

/// Path of the sync-fingerprint cache namespace. Reserved for Phase 5; the
/// test forge has no cache yet.
pub const TEST_CACHE_PATH: &str = "test_cache";

/// change id → stored change data (`test/`).
pub type ChangeDataByChange = HashMap<String, ChangeData>;

/// Test-forge-owned namespaces of a changes ref.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TestChangesRefData {
    #[serde(default)]
    pub test: ChangeDataByChange,
    #[serde(default)]
    pub test_admission: josh_changes::AdmissionData,
}

/// Store the data for a change: a sparse `TestChangesRefData` carrying only
/// the `test/<change-id>` entry, merged into the ref.
pub fn store_change_data(
    transaction: &josh_core::cache::Transaction,
    change_id: &str,
    data: &ChangeData,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<()> {
    let sparse = TestChangesRefData {
        test: [(change_id.to_string(), data.clone())].into(),
        ..Default::default()
    };
    josh_changes::write_filtered(
        transaction,
        scope,
        josh_changes::namespace_filter(TEST_CHANGE_DATA_PATH),
        &sparse,
        None,
        None,
    )?;
    Ok(())
}

/// Store the target branch's admission data: a sparse `TestChangesRefData`
/// carrying only the `test_admission/` subtree, merged into the ref.
pub fn store_admission_data(
    transaction: &josh_core::cache::Transaction,
    data: &josh_changes::AdmissionData,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<()> {
    let sparse = TestChangesRefData {
        test_admission: data.clone(),
        ..Default::default()
    };
    josh_changes::write_filtered(
        transaction,
        scope,
        josh_changes::namespace_filter(TEST_ADMISSION_PATH),
        &sparse,
        None,
        None,
    )?;
    Ok(())
}
