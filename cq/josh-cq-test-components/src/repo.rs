use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use tokio::sync::{mpsc, oneshot};
use url::Url;

use crate::actor::{self, ActorMsg};

const TEMP_DIR_PREFIX: &str = "josh-cq-test-components";
const HTTP_RECEIVE_PACK: &str = "http.receivepack";

pub struct TreeEntry {
    pub path: String,
    pub content: String,
}

pub enum TreeMode {
    Overlay(Vec<TreeEntry>),
    Replace(Vec<TreeEntry>),
}

pub struct TestRepoResources {
    _dir: tempfile::TempDir,
    _actor_handle: AbortOnDrop,
    _server_handle: AbortOnDrop,
}

pub struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct TestRepo {
    tx: mpsc::UnboundedSender<ActorMsg>,
    _guard: Arc<Mutex<TestRepoResources>>,
    path: PathBuf,
    url: Url,
}

impl TestRepo {
    pub async fn new() -> anyhow::Result<Self> {
        let dir = tempfile::Builder::new().prefix(TEMP_DIR_PREFIX).tempdir()?;

        let repo = git2::Repository::init_bare(dir.path())?;
        repo.set_head("refs/heads/main")?;
        repo.config()?.set_str(HTTP_RECEIVE_PACK, "true")?;
        drop(repo);

        let path = dir.path().to_owned();

        let (tx, rx) = mpsc::unbounded_channel::<ActorMsg>();

        let bind_addr = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let listener = tokio::net::TcpListener::bind(bind_addr).await?;
        let port = listener.local_addr()?.port();
        let url = Url::parse(&format!("http://{}:{port}/", Ipv4Addr::LOCALHOST))?;

        let app = axum::Router::new()
            .route("/{*path}", get(handle_git).post(handle_git))
            .with_state(tx.clone());

        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("axum server failed");
        });

        let repo_path = path.clone();
        let actor_handle = tokio::spawn(async move {
            actor::run_actor(rx, repo_path).await;
        });

        let guard = Arc::new(Mutex::new(TestRepoResources {
            _dir: dir,
            _actor_handle: AbortOnDrop(actor_handle),
            _server_handle: AbortOnDrop(server_handle),
        }));

        Ok(Self {
            tx,
            _guard: guard,
            path,
            url,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub async fn commit(
        &self,
        mode: TreeMode,
        message: &str,
        branch_ref: &str,
    ) -> anyhow::Result<git2::Oid> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(ActorMsg::Commit {
                mode,
                message: message.to_string(),
                branch_ref: branch_ref.to_string(),
                response: resp_tx,
            })
            .map_err(|_| anyhow::anyhow!("actor closed"))?;
        resp_rx.await?
    }

    pub async fn create_branch(&self, name: &str, from_ref: &str) -> anyhow::Result<git2::Oid> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(ActorMsg::CreateBranch {
                name: name.to_string(),
                from_ref: from_ref.to_string(),
                response: resp_tx,
            })
            .map_err(|_| anyhow::anyhow!("actor closed"))?;
        resp_rx.await?
    }

    pub async fn get_head(&self, branch_ref: &str) -> anyhow::Result<git2::Oid> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(ActorMsg::GetHead {
                branch_ref: branch_ref.to_string(),
                response: resp_tx,
            })
            .map_err(|_| anyhow::anyhow!("actor closed"))?;
        resp_rx.await?
    }

    pub fn into_parts(self) -> (PathBuf, Arc<Mutex<TestRepoResources>>) {
        (self.path, self._guard)
    }
}

async fn handle_git(
    State(tx): State<mpsc::UnboundedSender<ActorMsg>>,
    req: axum::extract::Request,
) -> Response<Body> {
    let (resp_tx, resp_rx) = oneshot::channel();
    if tx
        .send(ActorMsg::ServeGitHttp {
            request: req,
            response: resp_tx,
        })
        .is_err()
    {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("actor closed"))
            .expect("building error response");
    }
    resp_rx.await.unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("actor closed"))
            .expect("building error response")
    })
}
