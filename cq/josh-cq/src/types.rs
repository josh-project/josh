use serde::Deserialize;
use tokio::sync::oneshot;

use josh_github_webhooks::webhook_server::WebhookPayload;

#[derive(Deserialize)]
pub struct TrackRequest {
    pub url: String,
    pub id: String,
}

pub enum CqEvent {
    Track {
        request: TrackRequest,
        done: oneshot::Sender<()>,
    },
    Webhook(WebhookPayload),
    /// Periodic polling tick — triggers a full fetch + evaluate + step cycle.
    /// The optional oneshot fires after the queue cycle completes.
    Tick {
        done: Option<oneshot::Sender<()>>,
    },
}
