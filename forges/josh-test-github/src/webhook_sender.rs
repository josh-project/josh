use anyhow::Context;
use josh_github_webhooks::webhook_server::WebhookPayload;
use std::net::SocketAddr;

/// POST a webhook payload to the CQ server.
pub fn send_webhook(target: SocketAddr, payload: &WebhookPayload) -> anyhow::Result<()> {
    let tagged = serde_json::to_value(payload)?;
    let obj = tagged.as_object().context("Unexpected serialized format")?;
    let event_type = obj
        .get("type")
        .context("Missing type field")?
        .as_str()
        .context("Invalid type field")?;
    let data = obj.get("data").context("Missing data field")?;

    let url = format!("http://{}/v1/webhook", target);
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("X-GitHub-Event", event_type)
        .json(data)
        .send()?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "webhook POST failed: {}",
            response.status()
        ));
    }
    Ok(())
}
