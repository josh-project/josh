use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Basic},
};
use clap::Parser;

pub struct ServerState {
    users: Mutex<HashMap<String, String>>,
    args: Args,
}

#[derive(Clone, Parser)]
#[command(name = "axum-cgi-server")]
pub struct Args {
    #[arg(long)]
    dir: String,

    #[arg(long)]
    cmd: String,

    #[arg(long, short = 'a', num_args = 1..)]
    args: Vec<String>,

    #[arg(long, default_value = "8000")]
    port: String,

    #[arg(long)]
    proxy: Option<String>,
}

fn require_auth(
    auth: Option<TypedHeader<Authorization<Basic>>>,
    users: &HashMap<String, String>,
) -> Result<(), Box<Response>> {
    use http::header;

    if users.is_empty() {
        return Ok(());
    }

    let Some(TypedHeader(Authorization(basic))) = auth else {
        eprintln!("no credentials in request");

        let response = (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=User Visible Realm")],
        );

        return Err(response.into_response().into());
    };

    let username = basic.username();
    let password = basic.password();

    if let Some(stored_password) = users.get(username)
        && password == stored_password
    {
        eprintln!("CREDENTIALS OK {:?} {:?}", username, password);
        return Ok(());
    }

    eprintln!("ServerState: wrong user/pass");
    eprintln!("user: {:?}", username);
    eprintln!("pass: {:?}", password);

    let response = (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=User Visible Realm")],
    );

    Err(response.into_response().into())
}

fn make_password() -> String {
    use core::iter;
    use rand::{Rng, distr::Alphanumeric, rng};

    iter::repeat(())
        .map(|()| rng().sample(Alphanumeric))
        .map(char::from)
        .take(10)
        .collect()
}

async fn noauth_handler(State(state): State<Arc<ServerState>>) -> Response {
    eprintln!("handler: /_noauth");
    state.users.lock().unwrap().clear();
    StatusCode::OK.into_response()
}

async fn make_user_handler(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> Response {
    eprintln!("handler: /_make_user/{}", username);

    let mut users = state.users.lock().unwrap();
    if let Some(existing_password) = users.get(&username) {
        return existing_password.clone().into_response();
    }

    let password = make_password();
    users.insert(username, password.clone());

    eprintln!("users: {:?}", users);
    password.into_response()
}

async fn handle_proxy_request(
    proxy_target: &str,
    ppath: &str,
    req: Request,
) -> anyhow::Result<Response> {
    use axum::body::Body;

    let mut target_url = url::Url::parse(&format!("{}{}", proxy_target, ppath))?;

    if let Some(query) = req.uri().query() {
        target_url.set_query(Some(query));
    }

    let client = reqwest::Client::new();
    let mut proxy_req = client.request(req.method().clone(), target_url);

    // Copy headers
    for (key, value) in req.headers() {
        if let Ok(val) = value.to_str() {
            proxy_req = proxy_req.header(key.as_str(), val);
        }
    }

    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX).await?;
    proxy_req = proxy_req.body(body_bytes.to_vec());

    let response = proxy_req.send().await?;
    let status = response.status();
    let mut resp_builder = Response::builder().status(status);

    for (key, value) in response.headers() {
        resp_builder = resp_builder.header(key, value);
    }

    // Get response body
    let bytes = response.bytes().await?;
    Ok(resp_builder
        .body(Body::from(bytes.to_vec()))?
        .into_response())
}

async fn handler(
    State(state): State<Arc<ServerState>>,
    auth: Option<TypedHeader<Authorization<Basic>>>,
    req: Request,
) -> Response {
    let path = req.uri().path().to_string();
    eprintln!("handler: {:?}", path);

    if let Err(response) = require_auth(auth, &state.users.lock().unwrap()) {
        return response.into_response();
    }

    if let Some(proxy) = &state.args.proxy {
        for proxy_spec in proxy.split(',') {
            if let [proxy_path, proxy_target] = proxy_spec.split('=').collect::<Vec<_>>().as_slice()
                && let Some(ppath) = path.strip_prefix(proxy_path)
            {
                eprintln!("proxy {:?} -> {}{}", path, proxy_target, ppath);

                return handle_proxy_request(proxy_target, ppath, req)
                    .await
                    .unwrap_or_else(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Proxy error: {:?}", e),
                        )
                            .into_response()
                    });
            }
        }
    }

    let workdir = std::path::PathBuf::from(&state.args.dir);
    let mut cmd = tokio::process::Command::new(&state.args.cmd);

    for arg in &state.args.args {
        cmd.arg(arg);
    }
    cmd.current_dir(&workdir);
    cmd.env("PATH_INFO", &path);

    axum_cgi::do_cgi(req, cmd).await.0
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use axum::{
        Router,
        routing::{any, get},
    };
    use std::net::SocketAddr;

    let args = Args::parse();

    let server_state = Arc::new(ServerState {
        users: Default::default(),
        args,
    });

    let app = Router::new()
        .route("/_noauth", get(noauth_handler))
        .route("/_make_user/{username}", get(make_user_handler))
        .route("/{*path}", any(handler))
        .with_state(Arc::clone(&server_state));

    let addr: SocketAddr = format!("0.0.0.0:{}", server_state.args.port).parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("Now listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
