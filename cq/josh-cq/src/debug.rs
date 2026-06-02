//! Read-only introspection of the live actor state.
//!
//! Served through `GET /v1/debug?action=...&remote=...`. The handler runs inside
//! the actor loop, so it observes the same `CqActorState` the queue cycle uses —
//! no snapshots, no locks. Output is plaintext, intended for `curl` while
//! probing how the merge queue evaluates admission conditions against a real
//! GitHub remote.

use std::fmt::Write;

use josh_github_changes::admission::AdmissionState;
use josh_github_graphql::connection::GithubApiConnection;

use crate::api::get_or_fetch_maintainers;
use crate::git::{GitActor, GitActorMessage};
use crate::models::CqActorState;
use crate::types::{DebugAction, DebugRequest};

/// Dispatch a debug request to its handler, returning the plaintext body.
pub(crate) async fn handle_debug(
    req: &DebugRequest,
    git: &GitActor,
    api: &GithubApiConnection,
    state: &CqActorState,
) -> String {
    match req.action {
        DebugAction::GetAdmission => match &req.remote {
            Some(remote) => get_admission(remote, git, api, state).await,
            None => "error: action=get_admission requires &remote=<name>\n".to_string(),
        },
        DebugAction::ListRemotes => list_remotes(git).await,
    }
}

async fn list_remotes(git: &GitActor) -> String {
    let remotes = match git
        .request(|reply| GitActorMessage::ListTrackedRemotes { reply })
        .await
    {
        Ok(r) => r,
        Err(e) => return format!("error: failed to list tracked remotes: {e}"),
    };

    if remotes.is_empty() {
        return "no tracked remotes".to_string();
    }

    let mut out = String::new();
    for (name, meta) in remotes {
        let _ = writeln!(out, "{name:40} {}", meta.url);
    }
    out
}

/// Dump everything the queue knows about admission for the named remote: the
/// required checks resolved from rulesets, and every candidate PR with its
/// per-PR admission state.
async fn get_admission(
    remote: &str,
    git: &GitActor,
    api: &GithubApiConnection,
    state: &CqActorState,
) -> String {
    let remotes = match git
        .request(|reply| GitActorMessage::ListTrackedRemotes { reply })
        .await
    {
        Ok(r) => r,
        Err(e) => return format!("error: failed to list tracked remotes: {e}"),
    };

    let Some((_, meta)) = remotes.iter().find(|(name, _)| name == remote) else {
        let known: Vec<&str> = remotes.iter().map(|(n, _)| n.as_str()).collect();
        return format!(
            "error: no tracked remote named '{remote}'\nknown remotes: {}\n",
            if known.is_empty() {
                "(none)".to_string()
            } else {
                known.join(", ")
            }
        );
    };

    let url = &meta.url;

    let mut out = String::new();
    let _ = writeln!(out, "remote:    {remote}");
    let _ = writeln!(out, "url:       {url}");

    match state.resolve_owner_repo(url) {
        Some((owner, name)) => {
            let _ = writeln!(out, "owner/repo: {owner}/{name}");
        }
        None => {
            let _ = writeln!(out, "owner/repo: <unresolved>");
        }
    }

    out.push('\n');
    let _ = writeln!(out, "required checks (from active rulesets):");
    match state.required_checks.get(url) {
        Some(checks) if !checks.is_empty() => {
            for check in checks {
                let _ = writeln!(out, "  - {}", check.context);
            }
        }
        Some(_) => {
            let _ = writeln!(out, "  (none — rulesets define no required checks)");
        }
        None => {
            let _ = writeln!(out, "  (not fetched yet — no admission entry for this url)");
        }
    }

    // Maintainers are resolved live from the GitHub API (collaborators with
    // write access). This is the same call the queue uses to seed each PR's
    // admission state, so it shows who can approve even when no PR is open yet.
    out.push('\n');
    let _ = writeln!(out, "maintainers (write access, from GitHub API):");
    let mut maintainers = get_or_fetch_maintainers(state, url, api).await;
    maintainers.sort();
    if maintainers.is_empty() {
        let _ = writeln!(out, "  (none — or failed to fetch; check server logs)");
    } else {
        for login in &maintainers {
            let _ = writeln!(out, "  - {login}");
        }
    }

    let candidates: Vec<_> = state
        .candidates
        .values()
        .filter(|c| &c.repo_url == url)
        .collect();

    out.push('\n');
    let _ = writeln!(out, "candidate PRs: {}", candidates.len());
    for candidate in candidates {
        out.push('\n');
        let _ = writeln!(out, "PR #{} {:?}", candidate.number, candidate.title);
        let _ = writeln!(out, "  node_id:     {}", candidate.node_id);
        let _ = writeln!(out, "  head_sha:    {}", candidate.head_sha);
        let _ = writeln!(out, "  base_branch: {}", candidate.base_branch);
        let _ = writeln!(out, "  base_sha:    {}", candidate.base_sha);

        match state.admissions.get(&candidate.node_id) {
            Some(admission) => write_admission(&mut out, admission),
            None => {
                let _ = writeln!(out, "  admission:   <not initialized>");
            }
        }
    }

    out
}

/// Render a single PR's admission state: maintainers, reviews, required checks,
/// and the resulting `admissible()` verdict with a per-condition breakdown.
fn write_admission(out: &mut String, admission: &AdmissionState) {
    let _ = writeln!(out, "  admission:");

    let mut maintainers: Vec<&String> = admission.maintainers.iter().collect();
    maintainers.sort();
    let _ = writeln!(
        out,
        "    maintainers ({}): {}",
        maintainers.len(),
        if maintainers.is_empty() {
            "(none)".to_string()
        } else {
            maintainers
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );

    let _ = writeln!(out, "    maintainer reviews:");
    if admission.maintainer_reviews.is_empty() {
        let _ = writeln!(out, "      (none)");
    } else {
        for (login, review_state) in &admission.maintainer_reviews {
            let _ = writeln!(out, "      {login}: {review_state:?}");
        }
    }

    let _ = writeln!(out, "    required checks:");
    if admission.required_checks.is_empty() {
        let _ = writeln!(out, "      (none)");
    } else {
        for (check, passed) in &admission.required_checks {
            let _ = writeln!(
                out,
                "      {}: {}",
                check.context,
                if *passed { "PASS" } else { "pending/FAIL" }
            );
        }
    }

    // Mirror the conditions in AdmissionState::admissible() so it's clear which
    // one is blocking.
    let has_approval = admission.maintainer_reviews.values().any(|s| {
        matches!(
            s,
            josh_github_webhooks::webhook_types::PullRequestReviewState::Approved
        )
    });
    let no_changes_requested = admission.maintainer_reviews.values().all(|s| {
        !matches!(
            s,
            josh_github_webhooks::webhook_types::PullRequestReviewState::ChangesRequested
        )
    });
    let all_checks_passed = admission.required_checks.values().all(|&p| p);

    let _ = writeln!(out, "    conditions:");
    let _ = writeln!(out, "      has maintainer approval: {has_approval}");
    let _ = writeln!(out, "      no changes requested:    {no_changes_requested}");
    let _ = writeln!(out, "      all required checks pass: {all_checks_passed}");
    let _ = writeln!(out, "    => admissible: {}", admission.admissible());
}
