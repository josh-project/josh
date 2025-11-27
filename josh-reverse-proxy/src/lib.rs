// Inspired from https://github.com/felipenoris/hyper-reverse-proxy

use std::net::IpAddr;
use std::str::FromStr;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{CONTENT_LENGTH, HeaderMap, HeaderValue};
use hyper::http::header::{InvalidHeaderValue, ToStrError};
use hyper::http::uri::InvalidUri;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::Client as LegacyClient;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

const HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

#[derive(Debug)]
pub enum ProxyError {
    InvalidUri(InvalidUri),
    Hyper(hyper::Error),
    InvalidHeaderValue(InvalidHeaderValue),
    ForwardHeader(ToStrError),
    Client(hyper_util::client::legacy::Error),
}

type ProxyResult<T> = Result<T, ProxyError>;

impl From<hyper::Error> for ProxyError {
    fn from(err: hyper::Error) -> Self {
        ProxyError::Hyper(err)
    }
}

impl From<InvalidUri> for ProxyError {
    fn from(err: InvalidUri) -> Self {
        ProxyError::InvalidUri(err)
    }
}

impl From<InvalidHeaderValue> for ProxyError {
    fn from(err: InvalidHeaderValue) -> Self {
        ProxyError::InvalidHeaderValue(err)
    }
}

impl From<ToStrError> for ProxyError {
    fn from(err: ToStrError) -> Self {
        ProxyError::ForwardHeader(err)
    }
}

impl From<hyper_util::client::legacy::Error> for ProxyError {
    fn from(err: hyper_util::client::legacy::Error) -> Self {
        ProxyError::Client(err)
    }
}

fn is_hop_header(name: &str) -> bool {
    HOP_HEADERS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

fn remove_hop_headers(headers: &HeaderMap<HeaderValue>) -> HeaderMap<HeaderValue> {
    let mut result = HeaderMap::with_capacity(headers.len());
    for (key, value) in headers.iter() {
        if !is_hop_header(key.as_str()) {
            result.insert(key.clone(), value.clone());
        }
    }
    result
}

fn forward_uri(forward_url: &str, uri: &Uri) -> ProxyResult<Uri> {
    let target = match uri.query() {
        Some(query) => format!("{}{}?{}", forward_url, uri.path(), query),
        None => format!("{}{}", forward_url, uri.path()),
    };

    Ok(Uri::from_str(&target)?)
}

fn add_forwarded_for(headers: &mut HeaderMap<HeaderValue>, client_ip: IpAddr) -> ProxyResult<()> {
    use hyper::header::Entry;

    match headers.entry(hyper::header::HeaderName::from_static("x-forwarded-for")) {
        Entry::Vacant(entry) => {
            entry.insert(HeaderValue::from_str(&client_ip.to_string())?);
        }
        Entry::Occupied(mut entry) => {
            let addr = format!("{}, {}", entry.get().to_str()?, client_ip);
            entry.insert(HeaderValue::from_str(&addr)?);
        }
    }

    Ok(())
}

fn create_client() -> LegacyClient<HttpsConnector<HttpConnector>, Full<Bytes>> {
    let https = HttpsConnector::new();
    LegacyClient::builder(TokioExecutor::new()).build::<_, Full<Bytes>>(https)
}

async fn create_proxied_request(
    client_ip: IpAddr,
    forward_url: &str,
    request: Request<Incoming>,
) -> ProxyResult<Request<Full<Bytes>>> {
    let (mut parts, body) = request.into_parts();

    parts.headers = remove_hop_headers(&parts.headers);
    add_forwarded_for(&mut parts.headers, client_ip)?;
    parts.uri = forward_uri(forward_url, &parts.uri)?;

    let collected = body.collect().await?.to_bytes();
    let len = collected.len();

    if len == 0 {
        parts.headers.remove(CONTENT_LENGTH);
    } else {
        parts
            .headers
            .insert(CONTENT_LENGTH, HeaderValue::from_str(&len.to_string())?);
    }

    Ok(Request::from_parts(parts, Full::new(collected)))
}

async fn create_proxied_response(
    method: &Method,
    response: Response<Incoming>,
) -> ProxyResult<Response<Full<Bytes>>> {
    let (mut parts, body) = response.into_parts();
    parts.headers = remove_hop_headers(&parts.headers);

    if method == &Method::HEAD || parts.status == StatusCode::NO_CONTENT {
        let response = Response::from_parts(parts, Full::new(Bytes::new()));
        return Ok(response);
    }

    let bytes = body.collect().await?.to_bytes();
    let len = bytes.len();

    if len == 0 {
        parts.headers.remove(CONTENT_LENGTH);
    } else {
        parts
            .headers
            .insert(CONTENT_LENGTH, HeaderValue::from_str(&len.to_string())?);
    }

    Ok(Response::from_parts(parts, Full::new(bytes)))
}

pub async fn call(
    client_ip: IpAddr,
    forward_url: &str,
    request: Request<Incoming>,
) -> ProxyResult<Response<Full<Bytes>>> {
    let method = request.method().clone();
    let proxied_request = create_proxied_request(client_ip, forward_url, request).await?;

    let client = create_client();
    let response = client.request(proxied_request).await?;

    create_proxied_response(&method, response).await
}
