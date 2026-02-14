use http::StatusCode;
use serde::{Deserialize, Serialize};

pub const EVENT_HEADER: &str = "X-GitHub-Event";

fn make_tagged_payload(event: &str, payload: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": event,
        "data": payload,
    })
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type", content = "data")]
pub enum WebhookPayload {
    Ping(Box<crate::webhook_types::PingEvent>),
    Push(Box<crate::webhook_types::PushEvent>),
    PullRequest(Box<crate::webhook_types::PullRequestEvent>),
    WorkflowJob(Box<crate::webhook_types::WorkflowJobEvent>),
    WorkflowRun(Box<crate::webhook_types::WorkflowRunEvent>),
}

impl<S> axum::extract::FromRequest<S> for WebhookPayload
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let headers = req.headers().clone();
        let event = match headers.get(EVENT_HEADER) {
            Some(event) => event,
            None => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Unknown webhook operation".to_string(),
                ));
            }
        };

        let event = event
            .to_str()
            .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;

        let body = axum::body::Bytes::from_request(req, state)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("couldn't receive request body: {e}"),
                )
            })?;

        let payload: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("failed to parse request: {e}"),
            )
        })?;

        let payload = make_tagged_payload(event, payload);

        let payload = serde_json::from_value::<WebhookPayload>(payload).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("failed to parse request: {e}"),
            )
        })?;

        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use crate::webhook_server::{WebhookPayload, make_tagged_payload};
    use anyhow::Context;

    macro_rules! include_file {
        ($file:expr) => {
            ($file, include_str!($file))
        };
    }

    #[test]
    fn test_decode_json() -> anyhow::Result<()> {
        let values = [
            (include_file!("../tests/data/github_ping.json"), "ping"),
            (include_file!("../tests/data/github_push.json"), "push"),
            (
                include_file!("../tests/data/github_pull_request_merged.json"),
                "pull_request",
            ),
            (
                include_file!("../tests/data/github_pull_request_opened.json"),
                "pull_request",
            ),
            (
                include_file!("../tests/data/github_pull_request_synchronize.json"),
                "pull_request",
            ),
            (
                include_file!("../tests/data/github_pull_request_labeled.json"),
                "pull_request",
            ),
            (
                include_file!("../tests/data/github_pull_request_unlabeled.json"),
                "pull_request",
            ),
            (
                include_file!("../tests/data/github_workflow_job_completed.json"),
                "workflow_job",
            ),
            (
                include_file!("../tests/data/github_workflow_job_in_progress.json"),
                "workflow_job",
            ),
            (
                include_file!("../tests/data/github_workflow_job_queued.json"),
                "workflow_job",
            ),
            (
                include_file!("../tests/data/github_workflow_run_completed.json"),
                "workflow_run",
            ),
            (
                include_file!("../tests/data/github_workflow_run_in_progress.json"),
                "workflow_run",
            ),
            (
                include_file!("../tests/data/github_workflow_run_requested.json"),
                "workflow_run",
            ),
        ];

        for ((filename, json), event) in values {
            let payload: serde_json::Value = serde_json::from_str(json)
                .context("While decoding JSON")
                .context(filename)?;

            let tagged_payload = make_tagged_payload(event, payload);
            serde_json::from_value::<WebhookPayload>(tagged_payload).context(format!(
                "While decoding payload for file {} of type: {}",
                filename, event
            ))?;
        }

        Ok(())
    }
}
