use std::sync::Arc;

// Import the base64 crate Engine trait anonymously so we can
// call its methods without adding to the namespace.
use base64::engine::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tracing::Instrument;

// Auths in those groups are independent of each other.
// This lets us reduce mutex contention
#[derive(Hash, Eq, PartialEq, Clone)]
struct AuthTimersGroupKey {
    url: String,
    username: String,
}

impl AuthTimersGroupKey {
    fn new(url: &str, handle: &Handle) -> Self {
        let (username, _) = handle.parse().unwrap_or_default();

        Self {
            url: url.to_string(),
            username,
        }
    }
}

// Within a group, we can hold the lock for longer to verify the auth with upstream
type AuthTimersGroup = std::collections::HashMap<Handle, std::time::Instant>;
type AuthTimers =
    std::collections::HashMap<AuthTimersGroupKey, Arc<tokio::sync::Mutex<AuthTimersGroup>>>;

lazy_static! {
    // Note the use of std::sync::Mutex: access to those structures should only be performed
    // shortly, without blocking the async runtime for long time and without holding the
    // lock across an await point.
    static ref AUTH: std::sync::Mutex<std::collections::HashMap<Handle, Header>> = Default::default();
    static ref AUTH_TIMERS: std::sync::Mutex<AuthTimers> = Default::default();
}

// Wrapper struct for storing passwords to avoid having
// them output to traces by accident
#[derive(Clone, Default)]
struct Header {
    pub header: Option<hyper::header::HeaderValue>,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Handle {
    pub hash: Option<String>,
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle").field("value", &self.hash).finish()
    }
}

impl Handle {
    // Returns a pair: (username, password)
    pub fn parse(&self) -> Option<(String, String)> {
        let get_result = || -> josh::JoshResult<(String, String)> {
            let line = AUTH
                .lock()
                .unwrap()
                .get(self)
                .and_then(|h| h.header.as_ref())
                .map(|h| h.as_bytes().to_owned())
                .ok_or_else(|| josh::josh_error("no auth found"))?;

            let line = String::from_utf8(line)?;
            let (_, token) = line
                .split_once(' ')
                .ok_or_else(|| josh::josh_error("Unsupported auth type"))?;

            let decoded = BASE64.decode(token)?;
            let decoded = String::from_utf8(decoded)?;

            let (username, password) = decoded
                .split_once(':')
                .ok_or_else(|| josh::josh_error("No password found"))?;

            Ok((username.to_string(), password.to_string()))
        };

        match get_result() {
            Ok(pair) => Some(pair),
            Err(e) => {
                tracing::trace!(
                    handle = ?self,
                    "Falling back to default auth: {:?}",
                    e
                );

                None
            }
        }
    }
}

fn hash_header(header: &hyper::http::HeaderValue) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(header.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

pub fn add_auth(token: &str) -> josh::JoshResult<Handle> {
    let header = hyper::header::HeaderValue::from_str(&format!("Basic {}", BASE64.encode(token)))?;
    let handle = Handle {
        hash: Some(hash_header(&header)),
    };
    let header_wrapper = Header {
        header: Some(header),
    };
    AUTH.lock()?.insert(handle.clone(), header_wrapper);
    Ok(handle)
}

#[tracing::instrument()]
pub async fn check_http_auth(url: &str, auth: &Handle, required: bool) -> josh::JoshResult<bool> {
    use opentelemetry_semantic_conventions::trace::HTTP_RESPONSE_STATUS_CODE;

    if required && auth.hash.is_none() {
        return Ok(false);
    }

    let group_key = AuthTimersGroupKey::new(url, &auth);
    let auth_timers = AUTH_TIMERS
        .lock()
        .unwrap()
        .entry(group_key.clone())
        .or_default()
        .clone();

    let auth_header = AUTH.lock().unwrap().get(auth).cloned().unwrap_or_default();

    let refs_url = format!("{}/info/refs?service=git-upload-pack", url);
    let do_request = || {
        let refs_url = refs_url.clone();
        let do_request_span = tracing::info_span!("check_http_auth: make request");

        async move {
            let https = hyper_tls::HttpsConnector::new();
            let client = hyper::Client::builder().build::<_, hyper::Body>(https);

            let builder = hyper::Request::builder()
                .method(hyper::Method::GET)
                .uri(&refs_url);

            let builder = if let Some(value) = auth_header.header {
                builder.header(hyper::header::AUTHORIZATION, value)
            } else {
                builder
            };

            let request = builder.body(hyper::Body::empty())?;
            let resp = client.request(request).await?;

            Ok::<_, josh::JoshError>(resp)
        }
        .instrument(do_request_span)
    };

    // Only lock the mutex if auth handle is not empty, because otherwise
    // for remotes that require auth, we could run into situation where
    // multiple requests are executed essentially sequentially because
    // remote always returns 401 for authenticated requests and we never
    // populate the auth_timers map
    let resp = if auth.hash.is_some() {
        let mut auth_timers = auth_timers.lock().await;

        if let Some(last) = auth_timers.get(auth) {
            let since = std::time::Instant::now().duration_since(*last);
            let expired = since > std::time::Duration::from_secs(60 * 30);

            tracing::info!(
                last = ?last,
                since = ?since,
                expired = %expired,
                "check_http_auth: found auth entry"
            );

            if !expired {
                return Ok(true);
            }
        }

        tracing::info!(
            auth_timers_count = auth_timers.len(),
            "check_http_auth: no valid cached auth"
        );

        let resp = do_request().await?;
        if resp.status().is_success() {
            auth_timers.insert(auth.clone(), std::time::Instant::now());
        }

        resp
    } else {
        do_request().await?
    };

    let status = resp.status();

    tracing::event!(
        tracing::Level::INFO,
        { HTTP_RESPONSE_STATUS_CODE } = status.as_u16(),
        "check_http_auth: response"
    );

    if status == hyper::StatusCode::OK {
        Ok(true)
    } else if status == hyper::StatusCode::UNAUTHORIZED {
        tracing::event!(
            tracing::Level::WARN,
            { HTTP_RESPONSE_STATUS_CODE } = status.as_u16(),
            "check_http_auth: unauthorized"
        );

        let response = hyper::body::to_bytes(resp.into_body()).await?;
        let response = String::from_utf8_lossy(&response);

        tracing::event!(
            tracing::Level::TRACE,
            "http.response.body" = %response,
            "check_http_auth: unauthorized",
        );

        Ok(false)
    } else {
        return Err(josh::josh_error(&format!(
            "check_http_auth: got http response: {} {:?}",
            refs_url, resp
        )));
    }
}

pub fn strip_auth(
    req: hyper::Request<hyper::Body>,
) -> josh::JoshResult<(Handle, hyper::Request<hyper::Body>)> {
    let mut req = req;
    let header: Option<hyper::header::HeaderValue> =
        req.headers_mut().remove(hyper::header::AUTHORIZATION);

    if let Some(header) = header {
        let handle = Handle {
            hash: Some(hash_header(&header)),
        };
        let header_wrapper = Header {
            header: Some(header),
        };
        AUTH.lock()?.insert(handle.clone(), header_wrapper);
        return Ok((handle, req));
    }

    Ok((Handle { hash: None }, req))
}
