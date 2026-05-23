use serde::Deserialize;

use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;

pub(crate) const GH_TOKEN_ENV: &str = "GH_TOKEN";

#[derive(Deserialize)]
pub struct TrackRequest {
    pub url: String,
    pub id: String,
    #[serde(default = "crate::track::default_mode")]
    pub mode: String,
}

pub enum CqEvent {
    Track(TrackRequest),
    Webhook(WebhookPayload),
    /// Periodic polling tick — triggers a full fetch + evaluate + step cycle.
    Tick,
}

#[derive(Clone, Copy)]
pub(crate) enum AdmissionRelevantEvent<'a> {
    PullRequestReview(&'a webhook_types::PullRequestReviewEvent),
    CheckRun(&'a webhook_types::CheckRunEvent),
}

pub enum UserAction {
    Message(String),
}
