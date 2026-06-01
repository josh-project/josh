use serde::Deserialize;
use tokio::sync::oneshot;

use josh_github_webhooks::webhook_server::WebhookPayload;

#[derive(Deserialize)]
pub struct TrackRequest {
    pub url: String,
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugAction {
    GetAdmission,
    ListRemotes,
}

/// A read-only introspection request issued through `GET /v1/debug`. Carries the
/// parsed `action`/`remote` query parameters and a reply channel for the plaintext
/// response. Handled inline by the actor so it observes the live state without
/// locks; never triggers a queue cycle.
pub struct DebugRequest {
    pub action: DebugAction,
    pub remote: Option<String>,
    pub reply: oneshot::Sender<String>,
}

pub enum CqEvent {
    Track {
        request: TrackRequest,
        done: oneshot::Sender<anyhow::Result<()>>,
    },
    Webhook(WebhookPayload),
    /// Periodic polling tick — triggers a full fetch + evaluate + step cycle.
    /// The optional oneshot fires after the queue cycle completes.
    Tick {
        done: Option<oneshot::Sender<anyhow::Result<()>>>,
    },
    /// Read-only introspection of the live actor state (admission conditions,
    /// candidates, …). Replies with plaintext; does not mutate state.
    Debug(DebugRequest),
}
