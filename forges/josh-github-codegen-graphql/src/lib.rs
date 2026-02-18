#![allow(unused_imports)]
#![allow(clippy::needless_borrow)]

use chrono::{TimeZone, Utc};
use url::Url;

pub type Id = String;
pub type GitObjectId = String;
pub type NodeId = String;
pub type Uri = Url;
pub type DateTime = chrono::DateTime<Utc>;

// API operations (no fragments) - used for get_repo_id, create_pull_request, close_pull_request
#[allow(unused_imports)]
pub mod api {
    // Generated code uses super::Uri, super::Id etc.; super is this module, so re-export from crate
    pub use super::{DateTime, GitObjectId, Id, NodeId, Uri};
    include!(concat!(env!("OUT_DIR"), "/generated_api.rs"));
}
pub use api::{
    close_pull_request, convert_pull_request_to_draft, create_pull_request, get_default_branch,
    get_pr_by_head, get_repo_id, mark_pull_request_ready_for_review, update_pull_request,
    ClosePullRequest, ConvertPullRequestToDraft, CreatePullRequest, GetDefaultBranch, GetPrByHead,
    GetRepoId, MarkPullRequestReadyForReview, UpdatePullRequest,
};

include!(concat!(env!("OUT_DIR"), "/generated.rs"));
