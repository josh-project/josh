use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use futures::Stream;
use pin_project_lite::pin_project;

use std::pin::Pin;
use std::task::{Context, Poll};

// Wrapper to make anyhow::Error work with axum's IntoResponse
// while not running into coherence rules
//
// Converts to HTTP 500 so should be used for everything unexpected,
// errors that can occur during "normal" use should be handled
// by creating responses with respective codes explicitly
pub struct ProxyError(pub anyhow::Error);

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response<Body> {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

impl<E> From<E> for ProxyError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        ProxyError(err.into())
    }
}

pin_project! {
    pub struct StreamWithGuard<S, G> {
        #[pin]
        stream: S,
        _guard: G,
    }
}

impl<S, G> StreamWithGuard<S, G> {
    pub fn new(stream: S, guard: G) -> Self {
        Self {
            stream,
            _guard: guard,
        }
    }
}

impl<S: Stream, G> Stream for StreamWithGuard<S, G> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

impl<S, G> IntoResponse for StreamWithGuard<S, G>
where
    S: Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + 'static,
    G: Send + 'static,
{
    fn into_response(self) -> Response<Body> {
        Body::from_stream(self).into_response()
    }
}
