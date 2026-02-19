use std::sync::Arc;

use axum::extract;
use axum::http;
use axum::response::IntoResponse;
use secret_vault_value::SecretValue;

pub trait GithubWebhookSignature: Send + Sync {
    fn webhook_secret(&self) -> SecretValue;
}

pub async fn verify_github_signature_middleware(
    extract::State(state): extract::State<Arc<dyn GithubWebhookSignature>>,
    request: extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    // Extract the signature from headers
    let signature = match request.headers().get("X-Hub-Signature-256") {
        Some(sig) => sig.clone(),
        None => {
            tracing::warn!("missing X-Hub-Signature-256 header");
            return http::StatusCode::UNAUTHORIZED.into_response();
        }
    };

    let signature_str = match signature.to_str() {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!("invalid signature header encoding");
            return http::StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // GitHub sends the signature as "sha256=<hex>"
    let signature_hex = match signature_str.strip_prefix("sha256=") {
        Some(hex) => hex,
        None => {
            tracing::warn!("invalid signature format");
            return http::StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Extract the body
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!(error = ?e, "failed to read request body");
            return http::StatusCode::BAD_REQUEST.into_response();
        }
    };

    // Compute HMAC
    type HmacSha256 = Hmac<Sha256>;
    let webhook_secret = state.webhook_secret();

    let result = {
        let mut mac = match HmacSha256::new_from_slice(webhook_secret.as_sensitive_bytes()) {
            Ok(mac) => mac,
            Err(e) => {
                tracing::error!(error = ?e, "failed to create HMAC");
                return http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        mac.update(&bytes);
        mac.finalize()
    };

    let computed_signature = hex::encode(result.into_bytes());

    // Constant-time comparison
    if !constant_time_eq::constant_time_eq(signature_hex.as_bytes(), computed_signature.as_bytes())
    {
        tracing::warn!("invalid webhook signature");
        return http::StatusCode::UNAUTHORIZED.into_response();
    }

    // Reconstruct the request and pass it to the next handler
    let request = extract::Request::from_parts(parts, axum::body::Body::from(bytes));
    next.run(request).await
}
