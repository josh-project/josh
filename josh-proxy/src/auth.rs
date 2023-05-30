// Import the base64 crate Engine trait anonymously so we can
// call its methods without adding to the namespace.
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::engine::Engine as _;

lazy_static! {
    static ref AUTH: std::sync::Mutex<std::collections::HashMap<Handle, Header>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    static ref AUTH_TIMERS: std::sync::Mutex<AuthTimers> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

type AuthTimers = std::collections::HashMap<(String, Handle), std::time::Instant>;

// Wrapper struct for storing passwords to avoid having
// them output to traces by accident
#[derive(Clone)]
struct Header {
    pub header: Option<hyper::header::HeaderValue>,
}

#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Handle {
    pub hash: String,
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle").field("value", &self.hash).finish()
    }
}

impl Handle {
    // Returns a pair: (username, password)
    pub fn parse(&self) -> josh::JoshResult<(String, String)> {
        let line = josh::some_or!(
            AUTH.lock()
                .unwrap()
                .get(self)
                .and_then(|h| h.header.as_ref())
                .map(|h| h.as_bytes().to_owned()),
            {
                return Ok(("".to_string(), "".to_string()));
            }
        );

        let u = josh::ok_or!(String::from_utf8(line[6..].to_vec()), {
            return Ok(("".to_string(), "".to_string()));
        });
        let decoded = josh::ok_or!(BASE64.decode(u), {
            return Ok(("".to_string(), "".to_string()));
        });
        let s = josh::ok_or!(String::from_utf8(decoded), {
            return Ok(("".to_string(), "".to_string()));
        });
        let (username, password) = s.as_str().split_once(':').unwrap_or(("", ""));
        Ok((username.to_string(), password.to_string()))
    }
}

pub fn add_auth(token: &str) -> josh::JoshResult<Handle> {
    let header = hyper::header::HeaderValue::from_str(&format!("Basic {}", BASE64.encode(token)))?;
    let hp = Handle {
        hash: format!(
            "{:?}",
            git2::Oid::hash_object(git2::ObjectType::Blob, header.as_bytes())?
        ),
    };
    let p = Header {
        header: Some(header),
    };
    AUTH.lock()?.insert(hp.clone(), p);
    Ok(hp)
}

pub async fn check_auth(url: &str, auth: &Handle, required: bool) -> josh::JoshResult<bool> {
    if required && auth.hash.is_empty() {
        return Ok(false);
    }

    if let Some(last) = AUTH_TIMERS.lock()?.get(&(url.to_string(), auth.clone())) {
        let since = std::time::Instant::now().duration_since(*last);
        tracing::trace!("last: {:?}, since: {:?}", last, since);
        if since < std::time::Duration::from_secs(60 * 30) {
            tracing::trace!("cached auth");
            return Ok(true);
        }
    }

    tracing::trace!("no cached auth {:?}", *AUTH_TIMERS.lock()?);

    let https = hyper_tls::HttpsConnector::new();
    let client = hyper::Client::builder().build::<_, hyper::Body>(https);

    let password = AUTH
        .lock()?
        .get(auth)
        .unwrap_or(&Header { header: None })
        .to_owned();
    let refs_url = format!("{}/info/refs?service=git-upload-pack", url);

    let builder = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(&refs_url);

    let builder = if let Some(value) = password.header {
        builder.header(hyper::header::AUTHORIZATION, value)
    } else {
        builder
    };

    let request = builder.body(hyper::Body::empty())?;
    let resp = client.request(request).await?;

    let status = resp.status();

    tracing::trace!("http resp.status {:?}", resp.status());

    let err_msg = format!("got http response: {} {:?}", refs_url, resp);

    if status == hyper::StatusCode::OK {
        AUTH_TIMERS
            .lock()?
            .insert((url.to_string(), auth.clone()), std::time::Instant::now());
        Ok(true)
    } else if status == hyper::StatusCode::UNAUTHORIZED {
        tracing::warn!("resp.status == 401: {:?}", &err_msg);
        tracing::trace!(
            "body: {:?}",
            std::str::from_utf8(&hyper::body::to_bytes(resp.into_body()).await?)
        );
        Ok(false)
    } else {
        return Err(josh::josh_error(&err_msg));
    }
}

pub fn strip_auth(
    req: hyper::Request<hyper::Body>,
) -> josh::JoshResult<(Handle, hyper::Request<hyper::Body>)> {
    let mut req = req;
    let header: Option<hyper::header::HeaderValue> =
        req.headers_mut().remove(hyper::header::AUTHORIZATION);

    if let Some(header) = header {
        let hp = Handle {
            hash: format!(
                "{:?}",
                git2::Oid::hash_object(git2::ObjectType::Blob, header.as_bytes())?
            ),
        };
        let p = Header {
            header: Some(header),
        };
        AUTH.lock()?.insert(hp.clone(), p);
        return Ok((hp, req));
    }

    Ok((
        Handle {
            hash: "".to_owned(),
        },
        req,
    ))
}
