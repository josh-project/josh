use crate::webhook_types;

pub fn make_repository(clone_url: &str) -> webhook_types::Repository {
    webhook_types::Repository {
        clone_url: clone_url.to_string(),
        default_branch: "main".to_string(),
    }
}

pub fn make_pr_node_id(owner: &str, name: &str, number: usize) -> String {
    format!("PR_{}_{}_{}", owner, name, number)
}

pub fn make_pr_payload(
    owner: &str,
    name: &str,
    number: usize,
    head_ref: &str,
    head_sha: &str,
    base_ref: &str,
    base_sha: &str,
) -> webhook_types::PullRequest {
    let head_branch = head_ref.trim_start_matches("refs/heads/");
    let base_branch = base_ref.trim_start_matches("refs/heads/");

    webhook_types::PullRequest {
        node_id: make_pr_node_id(owner, name, number),
        number: number as i64,
        title: String::new(),
        body: None,
        created_at: Default::default(),
        updated_at: Default::default(),
        head: webhook_types::GitRef::new(head_branch, head_sha),
        base: webhook_types::GitRef::new(base_branch, base_sha),
        merged: None,
        merge_commit_sha: None,
        labels: vec![],
    }
}
