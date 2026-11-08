//! Synchronize change state from a test-forge remote.
//!
//! The test-forge analogue of the GitHub sync, minus the network beyond git
//! itself. The test forge is push-based: `josh changes publish` pushes the
//! branch as-is, so the whole stack lives on the remote's branch. Sync
//! fetches that branch (into the same `refs/josh/remotes/<name>/*` namespace
//! the remote-config refspec uses) plus the forge-state ref, walks the
//! branch's commits for `Change:` trailers (the same discovery
//! `josh changes sync` uses for the Local scope), and folds the `josh forge`
//! state tree into the forge-neutral `ChangeData`/`AdmissionData` shapes
//! from josh-changes, in the `test*` namespaces of
//! `refs/josh/remotes/<remote>/changes/<branch>`.
//!
//! There is no fingerprint cache and no GC yet (Phase 5): every sync
//! rewrites the stored data for every discovered change.

use std::collections::BTreeMap;

use anyhow::{Context, anyhow};
use josh_changes::{AdmissionData, ChangeData, RequiredStatusCheck};
use josh_core::cache::{Expected, Transaction};
use josh_core::objects;

use crate::layout::{store_admission_data, store_change_data};
use crate::{TEST_FORGE_REF, TestForgeState, read_state_at};

/// Where the remote's forge-state ref ([`TEST_FORGE_REF`]) lands locally when
/// fetched: `refs/josh/remotes/<remote>/forges/test`, inside the remote's
/// namespace like its unfiltered branch refs and changes refs.
pub fn local_state_ref(remote_name: &str) -> String {
    format!("refs/josh/remotes/{remote_name}/forges/test")
}

/// Options for a test-forge sync. `--no-cache`/`--cache-ttl` from the CLI are
/// accepted but meaningless: there is no cache to bypass or expire yet.
pub struct SyncOptions {
    /// Remove the remote's existing changes refs before syncing.
    pub clean: bool,
    /// Post local feedback to the forge; unsupported for the test forge.
    pub push: bool,
}

/// Synchronize a test-forge remote into its changes ref for `branch`.
/// `remote_name` must be a configured test-forge remote; `url` is its
/// configured fetch URL.
pub fn sync(
    transaction: &Transaction,
    remote_name: &str,
    branch: &str,
    url: &str,
    opts: SyncOptions,
) -> anyhow::Result<()> {
    if opts.push {
        return Err(anyhow!("--push is not supported for the test forge"));
    }

    let scope = josh_changes::ChangesRef::Remote {
        remote: remote_name.to_string(),
        branch: branch.to_string(),
    };

    if opts.clean {
        transaction.delete_ref(&scope.ref_name(), Expected::Any)?;
    }

    // Pull the branch into the remote's unfiltered namespace (the same spot
    // the remote-config refspec and `josh fetch` use) and the forge-state
    // ref alongside it.
    let branch_ref = format!("refs/josh/remotes/{remote_name}/{branch}");
    transaction
        .spawn_git(
            &[
                "fetch",
                "-q",
                url,
                "--no-tags",
                &format!("+refs/heads/{branch}:{branch_ref}"),
            ],
            &[],
        )
        .context("Failed to fetch the branch from the remote")?;

    let state_ref = local_state_ref(remote_name);
    let state = if remote_has_ref(transaction, url, TEST_FORGE_REF)? {
        transaction
            .spawn_git(
                &[
                    "fetch",
                    "-q",
                    url,
                    "--no-tags",
                    &format!("+{TEST_FORGE_REF}:{state_ref}"),
                ],
                &[],
            )
            .context("Failed to fetch the forge-state ref")?;
        read_state_at(transaction, &state_ref)?
    } else {
        // No `josh forge` state written yet: sync with empty state, and drop
        // any stale local copy from an earlier sync.
        transaction.delete_ref(&state_ref, Expected::Any)?;
        TestForgeState::default()
    };

    let tip = transaction
        .resolve_ref(&branch_ref)?
        .with_context(|| format!("{branch_ref} missing after fetch"))?;

    // Every commit with a Change: trailer on the branch is a change; each
    // change-id appears once, at its tip commit.
    let changes = josh_changes::get_change_tips(
        transaction,
        tip,
        gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
    )?;

    if changes.is_empty() {
        eprintln!("No published changes found on the test forge.");
        return Ok(());
    }
    eprintln!(
        "Found {} published changes on the test forge.",
        changes.len()
    );

    for change in &changes {
        let data = build_change_data(transaction, &state, change, branch)?;
        let change_id = change
            .id()
            .expect("get_change_tips only returns changes with an id");
        store_change_data(transaction, change_id, &data, &scope)?;
    }

    let data = build_admission_data(&state, branch)?;
    store_admission_data(transaction, &data, &scope)?;

    Ok(())
}

/// Does the remote currently have `ref_name`? Used to tolerate a missing
/// forge-state ref without a failing (and noisy) fetch.
fn remote_has_ref(transaction: &Transaction, url: &str, ref_name: &str) -> anyhow::Result<bool> {
    let output = transaction
        .git_command(&["ls-remote", url, ref_name], &[])?
        .spawn()
        .context("Failed to query remote refs")?;
    Ok(output.status.success() && !output.stdout.is_empty())
}

/// Fold one discovered change and the forge state into the `ChangeData`
/// shape the read path consumes. The test forge has no PRs, so the
/// PR-specific fields are `None` and absent from the stored tree, and no
/// diff stats are computed. Being push-based, there is no per-change ref
/// either: `head_ref_name` and `base_ref_name` are both simply the branch.
fn build_change_data(
    transaction: &Transaction,
    state: &TestForgeState,
    change: &josh_changes::Change,
    branch: &str,
) -> anyhow::Result<ChangeData> {
    let head_oid = change.commit();
    let commit = objects::CommitData::read(transaction.odb(), head_oid)?;
    let parsed = commit.parsed()?;
    let message = std::str::from_utf8(commit.message()?.as_ref())
        .unwrap_or("")
        .trim_end();
    let mut lines = message.lines();
    let title = lines.next().unwrap_or("").trim().to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let timestamp = rfc3339(parsed.author()?.time().map(|t| t.seconds).unwrap_or(0));
    let change_id = change.id().expect("caller filters to changes with an id");

    Ok(ChangeData {
        title,
        body: (!body.is_empty()).then_some(body),
        number: None,
        url: None,
        state: "Open".to_string(),
        is_draft: None,
        author: change.author().to_string(),
        created_at: timestamp.clone(),
        updated_at: timestamp,
        merged: None,
        merged_at: None,
        merged_by: None,
        additions: 0,
        deletions: 0,
        changed_files: 0,
        base_ref_name: branch.to_string(),
        head_ref_name: branch.to_string(),
        reviews: state.reviews.get(change_id).cloned().unwrap_or_default(),
        checks: state
            .checks
            .get(&head_oid.to_string())
            .cloned()
            .unwrap_or_default(),
        labels: Vec::new(),
        comments: Vec::new(),
    })
}

/// Build one target branch's `AdmissionData` from the forge state: the
/// maintainer set as-is, the branch's required-check markers as
/// `RequiredStatusCheck`s (no integration id), and the decimal-string
/// approvals bar parsed back to a number. `fetched_at` follows josh's actor
/// time, so tests pinning `JOSH_COMMIT_TIME` get a deterministic value.
fn build_admission_data(state: &TestForgeState, branch: &str) -> anyhow::Result<AdmissionData> {
    let branch_admission = state.admission.get(branch);
    let required_checks: BTreeMap<String, RequiredStatusCheck> = branch_admission
        .map(|admission| {
            admission
                .required_checks
                .keys()
                .map(|name| {
                    (
                        name.clone(),
                        RequiredStatusCheck {
                            context: name.clone(),
                            integration_id: None,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let required_approvals = match branch_admission {
        Some(admission) if !admission.required_approvals.is_empty() => {
            admission.required_approvals.parse::<u32>().map_err(|_| {
                anyhow!(
                    "invalid required_approvals '{}' for branch '{branch}' in the forge state",
                    admission.required_approvals
                )
            })?
        }
        _ => 0,
    };

    Ok(AdmissionData {
        fetched_at: josh_core::git::josh_actor_signature()?.time.seconds.max(0) as u64,
        maintainers: state.maintainers.clone(),
        required_checks,
        required_approvals,
    })
}

fn rfc3339(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}
