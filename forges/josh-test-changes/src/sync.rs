//! Synchronize change state from a test-forge remote.
//!
//! The test forge is push-based: `josh changes publish` pushes the branch
//! as-is, so the whole stack lives on the remote's branch. Sync fetches that
//! branch (into the same `refs/josh/remotes/<name>/*` namespace the
//! remote-config refspec uses) plus the forge-state ref, walks the branch's
//! commits for `Change:` trailers (the same discovery `josh changes sync`
//! uses for the Local scope), and folds the `josh forge` state tree into the
//! forge-neutral `ChangeData`/`AdmissionData` shapes from josh-changes, in
//! the `test*` namespaces of `refs/josh/remotes/<remote>/changes/<branch>`.
//!
//! Sync manages the store the way the GitHub sync does: entries present in
//! the fresh state overlay, entries absent from it are retracted with a
//! single exclude-filter deletion.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, anyhow};
use josh_changes::{AdmissionData, ChangeData, RequiredStatusCheck};
use josh_core::cache::{Expected, Transaction};
use josh_core::objects;

use crate::layout::{
    ChangeDataByChange, TEST_ADMISSION_PATH, TEST_CHANGE_DATA_PATH, read_admission_data,
    read_all_change_data, store_admission_data, store_change_data,
};
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
    let state_present = remote_has_ref(transaction, url, TEST_FORGE_REF)?;
    let state = if state_present {
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

    // Build the fresh per-change data from the discovered changes and the
    // fetched forge state.
    let mut fresh: ChangeDataByChange = ChangeDataByChange::default();
    for change in &changes {
        let change_id = change
            .id()
            .expect("get_change_tips only returns changes with an id");
        fresh.insert(
            change_id.to_string(),
            build_change_data(transaction, &state, change, branch)?,
        );
    }
    // The fresh admission data; None when it should be absent from the ref:
    // either the remote has no forge-state ref (mergeability degrades to
    // "unknown", like the GitHub path with a failed admission fetch) or the
    // branch carries no changes at all.
    let fresh_admission = if state_present && !changes.is_empty() {
        Some(build_admission_data(&state, branch)?)
    } else {
        None
    };

    // Retract whatever the fresh state no longer carries -- subtraction
    // where the writes below overlay, mirroring how the GitHub sync
    // garbage-collects its store (josh_changes::delete_change), plus
    // per-key retraction within surviving entries, which the GitHub path
    // does not need (its per-PR entries are replaced wholesale on fetch).
    let old_data = read_all_change_data(transaction, &scope)?;
    let old_admission = read_admission_data(transaction, &scope)?;
    let stored_ids: BTreeSet<String> = josh_changes::list_changes(transaction, &scope)?
        .into_iter()
        .filter_map(|change| change.id().map(str::to_string))
        .chain(old_data.keys().cloned())
        .collect();
    let delete_paths = retraction_paths(
        &stored_ids,
        &old_data,
        old_admission.as_ref(),
        &fresh,
        fresh_admission.as_ref(),
    );
    if !delete_paths.is_empty() {
        josh_changes::delete_filtered(transaction, &delete_paths, &scope)?;
    }

    if changes.is_empty() {
        eprintln!("No published changes found on the test forge.");
        return Ok(());
    }
    eprintln!(
        "Found {} published changes on the test forge.",
        changes.len()
    );

    for change in &changes {
        let change_id = change
            .id()
            .expect("get_change_tips only returns changes with an id");
        let data = fresh
            .get(change_id)
            .expect("built for every discovered change");
        store_change_data(transaction, change_id, data, &scope)?;
        // Populate diffs/ too: `josh changes list --remote` enumerates
        // changes from it, like it does for the Local scope.
        josh_changes::store_diff_data(transaction, change, &scope)?;
    }

    if let Some(data) = &fresh_admission {
        store_admission_data(transaction, data, &scope)?;
    }

    Ok(())
}

/// The paths to delete from the changes ref before overlaying the fresh
/// state: every stored change that vanished from the branch (the full
/// per-change path set, as in `josh_changes::delete_change`), plus, for
/// surviving changes and the admission data, the individual check/review/
/// required-check keys the fresh state no longer carries. A `None`
/// `fresh_admission` retracts the whole `test_admission/` subtree, and so
/// does a shrunken maintainer set: maintainer markers are empty trees,
/// which the exclude-filter machinery cannot remove by path, so the subtree
/// is dropped and re-stored instead.
fn retraction_paths(
    stored_ids: &BTreeSet<String>,
    old_data: &ChangeDataByChange,
    old_admission: Option<&AdmissionData>,
    fresh: &ChangeDataByChange,
    fresh_admission: Option<&AdmissionData>,
) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();

    for change_id in stored_ids {
        if !fresh.contains_key(change_id) {
            paths.extend(josh_changes::change_paths(
                change_id,
                &[TEST_CHANGE_DATA_PATH],
            ));
        }
    }
    for (change_id, old) in old_data {
        let Some(new) = fresh.get(change_id) else {
            continue;
        };
        let encoded = josh_changes::encode_change_id_path(change_id);
        for name in old.checks.keys() {
            if !new.checks.contains_key(name) {
                paths.push(
                    std::path::Path::new(TEST_CHANGE_DATA_PATH)
                        .join(&encoded)
                        .join("checks")
                        .join(josh_git_serde::encode_key(name)),
                );
            }
        }
        for user in old.reviews.keys() {
            if !new.reviews.contains_key(user) {
                paths.push(
                    std::path::Path::new(TEST_CHANGE_DATA_PATH)
                        .join(&encoded)
                        .join("reviews")
                        .join(josh_git_serde::encode_key(user)),
                );
            }
        }
    }

    match (old_admission, fresh_admission) {
        (Some(old), Some(new)) => {
            if old
                .maintainers
                .keys()
                .any(|user| !new.maintainers.contains_key(user))
            {
                paths.push(std::path::PathBuf::from(TEST_ADMISSION_PATH));
            } else {
                for name in old.required_checks.keys() {
                    if !new.required_checks.contains_key(name) {
                        paths.push(
                            std::path::Path::new(TEST_ADMISSION_PATH)
                                .join("required_checks")
                                .join(josh_git_serde::encode_key(name)),
                        );
                    }
                }
            }
        }
        (Some(_), None) => paths.push(std::path::PathBuf::from(TEST_ADMISSION_PATH)),
        (None, _) => {}
    }

    paths
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
/// PR-specific fields are `None` and absent from the stored tree; being
/// push-based, there is no per-change ref either, so `head_ref_name` and
/// `base_ref_name` are both simply the branch. No diff stats are computed.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CheckState, ReviewState};

    fn change_data(reviews: &[(&str, ReviewState)], checks: &[(&str, CheckState)]) -> ChangeData {
        ChangeData {
            title: String::new(),
            body: None,
            number: None,
            url: None,
            state: "Open".to_string(),
            is_draft: None,
            author: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            merged: None,
            merged_at: None,
            merged_by: None,
            additions: 0,
            deletions: 0,
            changed_files: 0,
            base_ref_name: String::new(),
            head_ref_name: String::new(),
            reviews: reviews
                .iter()
                .map(|(user, state)| (user.to_string(), state.clone()))
                .collect(),
            checks: checks
                .iter()
                .map(|(name, state)| (name.to_string(), *state))
                .collect(),
            labels: Vec::new(),
            comments: Vec::new(),
        }
    }

    fn admission(maintainers: &[&str], required: &[&str]) -> AdmissionData {
        AdmissionData {
            fetched_at: 0,
            maintainers: maintainers
                .iter()
                .map(|user| (user.to_string(), ()))
                .collect(),
            required_checks: required
                .iter()
                .map(|context| {
                    (
                        context.to_string(),
                        RequiredStatusCheck {
                            context: context.to_string(),
                            integration_id: None,
                        },
                    )
                })
                .collect(),
            required_approvals: 0,
        }
    }

    fn paths_sorted(paths: &[std::path::PathBuf]) -> Vec<String> {
        let mut paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_str().unwrap().to_string())
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn retraction_covers_vanished_and_shrunken_state() {
        let mut old_data = ChangeDataByChange::default();
        old_data.insert(
            "gone".to_string(),
            change_data(&[("alice", ReviewState::Approved)], &[]),
        );
        old_data.insert(
            "keep".to_string(),
            change_data(
                &[
                    ("alice", ReviewState::Approved),
                    ("bob", ReviewState::Commented),
                ],
                &[
                    ("build", CheckState::Success),
                    ("lint", CheckState::Pending),
                ],
            ),
        );
        let old_admission = admission(&["alice", "bob"], &["build", "test"]);

        let mut fresh = ChangeDataByChange::default();
        fresh.insert(
            "keep".to_string(),
            change_data(
                &[("alice", ReviewState::Approved)],
                &[("build", CheckState::Success)],
            ),
        );
        let fresh_admission = admission(&["alice"], &["build"]);

        let stored_ids: BTreeSet<String> = old_data.keys().cloned().collect();
        let paths = paths_sorted(&retraction_paths(
            &stored_ids,
            &old_data,
            Some(&old_admission),
            &fresh,
            Some(&fresh_admission),
        ));

        // The vanished change retracts its full per-change path set;
        // surviving entries retract only the keys the fresh state dropped.
        // A shrunken maintainer set retracts the whole admission subtree
        // (empty-tree markers cannot be excluded by path).
        assert_eq!(
            paths,
            vec![
                "comments/gone",
                "diffs/gone",
                "outbox/comments/gone",
                "outbox/votes/gone",
                "test/gone",
                "test/keep/checks/lint",
                "test/keep/reviews/bob",
                "test_admission",
                "votes/gone",
            ]
        );
    }

    #[test]
    fn required_check_shrink_retracts_by_key() {
        let old_admission = admission(&["alice"], &["build", "test"]);
        let fresh_admission = admission(&["alice"], &["build"]);
        let paths = retraction_paths(
            &BTreeSet::new(),
            &ChangeDataByChange::default(),
            Some(&old_admission),
            &ChangeDataByChange::default(),
            Some(&fresh_admission),
        );
        assert_eq!(
            paths,
            vec![std::path::PathBuf::from(
                "test_admission/required_checks/test"
            )]
        );
    }

    #[test]
    fn absent_fresh_admission_retracts_the_whole_subtree() {
        let old_admission = admission(&["alice"], &["build"]);
        let paths = retraction_paths(
            &BTreeSet::new(),
            &ChangeDataByChange::default(),
            Some(&old_admission),
            &ChangeDataByChange::default(),
            None,
        );
        assert_eq!(paths, vec![std::path::PathBuf::from("test_admission")]);

        // Nothing stored, nothing to retract.
        assert!(
            retraction_paths(
                &BTreeSet::new(),
                &ChangeDataByChange::default(),
                None,
                &ChangeDataByChange::default(),
                None,
            )
            .is_empty()
        );
    }
}
