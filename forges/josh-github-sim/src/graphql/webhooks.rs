use url::Url;

use super::types::MockPr;

pub(crate) fn build_pr_opened_event(
    owner: &str,
    name: &str,
    pr: &MockPr,
    sim_url: &Url,
) -> serde_json::Value {
    let clone_url = sim_url
        .join(&format!("{}/{}", owner, name))
        .map(|u| u.to_string())
        .unwrap_or_default();
    build_pr_event("opened", pr, &clone_url)
}

pub(crate) fn build_pr_closed_event(
    owner: &str,
    name: &str,
    pr: &MockPr,
    sim_url: &Url,
) -> serde_json::Value {
    let clone_url = sim_url
        .join(&format!("{}/{}", owner, name))
        .map(|u| u.to_string())
        .unwrap_or_default();
    build_pr_event("closed", pr, &clone_url)
}

fn build_pr_event(action: &str, pr: &MockPr, clone_url: &str) -> serde_json::Value {
    let head_ref = pr.head_ref_name.trim_start_matches("refs/heads/");
    let base_ref = pr.base_ref_name.trim_start_matches("refs/heads/");
    serde_json::json!({
        "action": action,
        "pull_request": {
            "node_id": pr.node_id,
            "number": pr.number,
            "title": pr.title,
            "body": null,
            "created_at": "1970-01-01T00:00:00Z",
            "updated_at": "1970-01-01T00:00:00Z",
            "head": {
                "ref": head_ref,
                "sha": pr.head_ref_oid,
            },
            "base": {
                "ref": base_ref,
                "sha": pr.base_ref_oid,
            },
            "merged": null,
            "merge_commit_sha": null,
            "labels": [],
        },
        "repository": {
            "clone_url": clone_url,
            "default_branch": "main",
        },
    })
}

pub(crate) fn build_check_run_event(
    name: &str,
    head_sha: &str,
    conclusion: &str,
    clone_url: &str,
) -> serde_json::Value {
    serde_json::json!({
        "action": "completed",
        "check_run": {
            "id": 1,
            "name": name,
            "head_sha": head_sha,
            "status": "completed",
            "conclusion": conclusion,
            "started_at": "1970-01-01T00:00:00Z",
            "completed_at": null,
        },
        "repository": {
            "clone_url": clone_url,
            "default_branch": "main",
        },
    })
}

pub(crate) fn build_pr_review_event(
    pr: &MockPr,
    reviewer: &str,
    state: &str,
    clone_url: &str,
) -> serde_json::Value {
    serde_json::json!({
        "action": "submitted",
        "review": {
            "id": 1,
            "user": { "login": reviewer },
            "body": null,
            "commit_id": pr.head_ref_oid,
            "submitted_at": "1970-01-01T00:00:00Z",
            "state": state,
        },
        "pull_request": {
            "node_id": pr.node_id,
            "number": pr.number,
            "title": pr.title,
            "body": null,
            "created_at": "1970-01-01T00:00:00Z",
            "updated_at": "1970-01-01T00:00:00Z",
            "head": {
                "ref": pr.head_ref_name.trim_start_matches("refs/heads/"),
                "sha": pr.head_ref_oid,
            },
            "base": {
                "ref": pr.base_ref_name.trim_start_matches("refs/heads/"),
                "sha": pr.base_ref_oid,
            },
            "merged": null,
            "merge_commit_sha": null,
            "labels": [],
        },
        "repository": {
            "clone_url": clone_url,
            "default_branch": "main",
        },
    })
}

pub(crate) async fn send_webhook(wh_url: &Url, event_type: &str, body: serde_json::Value) {
    let target = wh_url.join("/v1/webhook").unwrap();
    let client = reqwest::Client::new();
    if let Err(e) = client
        .post(target)
        .header("X-GitHub-Event", event_type)
        .json(&body)
        .send()
        .await
    {
        tracing::error!(error = ?e, "failed to post webhook to CQ");
    }
}
