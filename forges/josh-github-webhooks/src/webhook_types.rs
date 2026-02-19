use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Common repository information shared across webhook events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub clone_url: String,
    pub default_branch: String,
}

/// Common user information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub login: String,
}

/// Reference information for pull requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRef {
    sha: String,

    // It's called "ref" but it's actually a branch, so we hide it
    // behind a "getter" that will prepend "refs/heads"
    #[serde(rename = "ref")]
    branch: String,
}

impl GitRef {
    pub fn new(ref_: impl AsRef<str>, sha: impl AsRef<str>) -> GitRef {
        GitRef {
            sha: sha.as_ref().to_string(),
            branch: ref_.as_ref().to_string(),
        }
    }

    pub fn sha(&self) -> String {
        self.sha.clone()
    }

    pub fn reference(&self) -> String {
        format!("refs/heads/{}", self.branch)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
}

/// Core pull request data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub head: GitRef,
    pub base: GitRef,
    pub merged: Option<bool>,
    pub merge_commit_sha: Option<String>,
    pub labels: Vec<Label>,
}

/// Ping event sent when webhook is first configured
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingEvent {
    pub zen: String,
    pub hook_id: i64,
    pub repository: Repository,
}

/// Push event for commits to branches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushEvent {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub before: String,
    pub after: String,
    pub repository: Repository,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PullRequestEventDetails {
    Opened,
    Synchronize { before: String, after: String },
    Closed,
    Labeled,
    Unlabeled,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestEvent {
    pub pull_request: PullRequest,
    pub repository: Repository,

    #[serde(flatten)]
    pub details: PullRequestEventDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowJobConclusion {
    Success,
    Failure,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowJob {
    pub id: i64,
    pub name: String,
    pub head_sha: String,
    pub head_branch: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub conclusion: Option<WorkflowJobConclusion>,
    pub workflow_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WorkflowJobEventDetails {
    Queued,
    InProgress,
    Completed,
    Waiting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowJobEvent {
    pub workflow_job: WorkflowJob,
    pub repository: Repository,

    #[serde(flatten)]
    pub details: WorkflowJobEventDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: i64,
    pub name: String,
    pub head_sha: String,
    pub head_branch: Option<String>,
    pub status: String,
    pub conclusion: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WorkflowRunEventDetails {
    Requested,
    InProgress,
    Completed,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub struct WorkflowRunEvent {
    pub workflow_run: WorkflowRun,
    pub repository: Repository,

    #[serde(flatten)]
    pub details: WorkflowRunEventDetails,
}
