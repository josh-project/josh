use axum::http::Response;
use opentelemetry::global;
use tower_http::trace::{MakeSpan, OnResponse};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use std::collections::HashMap;
use std::time::Duration;

pub fn make_context_propagator() -> HashMap<String, String> {
    let span = Span::current();

    let mut context_propagator = HashMap::<String, String>::default();
    let context = span.context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut context_propagator);
    });

    tracing::debug!("context propagator: {:?}", context_propagator);
    context_propagator
}

// Re-implemented here instead of using DefaultMakeSpan so that
// the span target could be explicitly overridden here and set
// to `josh-proxy`
#[derive(Clone)]
pub struct SpanMaker {}

impl<B> MakeSpan<B> for SpanMaker {
    fn make_span(&mut self, request: &axum::http::Request<B>) -> Span {
        use opentelemetry_semantic_conventions::trace::*;

        tracing::span!(
            target: "josh_proxy",
            tracing::Level::INFO,
            "http_request",
            { HTTP_REQUEST_METHOD } = %request.method(),
            { URL_PATH } = %request.uri().path(),
            { HTTP_RESPONSE_STATUS_CODE } = tracing::field::Empty,
        )
    }
}

#[derive(Clone)]
pub struct TraceResponse {}

impl<B> OnResponse<B> for TraceResponse {
    fn on_response(self, response: &Response<B>, _: Duration, span: &Span) {
        use opentelemetry_semantic_conventions::trace::*;

        // Record value of previously allocated field
        span.record(HTTP_RESPONSE_STATUS_CODE, response.status().as_u16());
    }
}
