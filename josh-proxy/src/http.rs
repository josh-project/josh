use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use josh_core::JoshError;

// Wrapper to make JoshError work with axum's IntoResponse
// while not running into coherence rules
//
// Converts to HTTP 500 so should be used for everything unexpected,
// errors that can occur during "normal" use should be handled
// by creating responses with respective codes explicitly
pub struct ProxyError(pub JoshError);

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response<Body> {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.0).into_response()
    }
}

impl<E> From<E> for ProxyError
where
    E: Into<JoshError>,
{
    fn from(err: E) -> Self {
        ProxyError(err.into())
    }
}
