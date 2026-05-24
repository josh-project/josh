use std::collections::{BTreeMap, HashMap};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use tokio::sync::{mpsc, oneshot};
use url::Url;

use josh_cq_test_components::TestRepo;
use josh_cq_test_components::repo::TestRepoResources;

use crate::actor::{self, ActorMsg};
use crate::graphql::{GraphQLState, RepoState};
use crate::{MockPr, MockRuleset};

pub struct RepoConfig {
    pub owner: String,
    pub name: String,
    pub repo: TestRepo,
}

struct GithubSimResources {
    _repo_guards: Vec<Arc<Mutex<TestRepoResources>>>,
    _actor_handle: AbortOnDrop,
    _server_handle: AbortOnDrop,
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct GithubSim {
    _tx: mpsc::UnboundedSender<ActorMsg>,
    _guard: Arc<Mutex<GithubSimResources>>,
    url: Url,
    graphql_url: Url,
    graphql_state: Arc<Mutex<GraphQLState>>,
}

impl GithubSim {
    pub async fn new(repos: Vec<RepoConfig>) -> anyhow::Result<Self> {
        let mut repo_map: HashMap<(String, String), PathBuf> = HashMap::new();
        let mut repos_state: HashMap<(String, String), RepoState> = HashMap::new();
        let mut guards: Vec<Arc<Mutex<TestRepoResources>>> = Vec::new();

        for config in repos {
            repos_state.insert(
                (config.owner.clone(), config.name.clone()),
                RepoState {
                    prs: Vec::new(),
                    reviews: BTreeMap::new(),
                    maintainers: Vec::new(),
                    rulesets: Vec::new(),
                    closed_prs: Vec::new(),
                    comments: Vec::new(),
                },
            );
            let (path, guard) = config.repo.into_parts();
            repo_map.insert((config.owner, config.name), path);
            guards.push(guard);
        }

        let (tx, rx) = mpsc::unbounded_channel::<ActorMsg>();

        let bind_addr = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let listener = tokio::net::TcpListener::bind(bind_addr).await?;
        let port = listener.local_addr()?.port();
        let url = Url::parse(&format!("http://{}:{port}/", Ipv4Addr::LOCALHOST))?;
        let graphql_url = Url::parse(&format!("http://{}:{port}/graphql", Ipv4Addr::LOCALHOST))?;
        let sim_url = url.clone();

        let graphql_state = Arc::new(Mutex::new(GraphQLState {
            repos: repos_state,
            webhook_url: None,
            sim_url: Some(sim_url),
        }));

        let app = axum::Router::new()
            .route("/graphql", post(handle_graphql))
            .route("/{*path}", get(handle_git).post(handle_git))
            .with_state(tx.clone());

        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("axum server failed");
        });

        let actor_state = graphql_state.clone();
        let actor_handle = tokio::spawn(async move {
            actor::run_actor(rx, repo_map, actor_state).await;
        });

        let guard = Arc::new(Mutex::new(GithubSimResources {
            _repo_guards: guards,
            _actor_handle: AbortOnDrop(actor_handle),
            _server_handle: AbortOnDrop(server_handle),
        }));

        Ok(Self {
            _tx: tx,
            _guard: guard,
            url,
            graphql_url,
            graphql_state,
        })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn graphql_url(&self) -> &Url {
        &self.graphql_url
    }

    pub fn graphql_state(&self) -> &Arc<Mutex<GraphQLState>> {
        &self.graphql_state
    }

    pub fn closed_pr_node_ids(&self) -> Vec<String> {
        self.graphql_state
            .lock()
            .unwrap()
            .repos
            .values()
            .flat_map(|r| r.closed_prs.iter())
            .cloned()
            .collect()
    }

    pub fn comments(&self) -> Vec<(String, String)> {
        self.graphql_state
            .lock()
            .unwrap()
            .repos
            .values()
            .flat_map(|r| r.comments.iter())
            .cloned()
            .collect()
    }

    pub fn set_webhook_url(&self, url: Url) {
        self.graphql_state.lock().unwrap().webhook_url = Some(url);
    }

    async fn send_msg<R>(&self, msg: ActorMsg, rx: oneshot::Receiver<R>) -> anyhow::Result<R> {
        self._tx
            .send(msg)
            .map_err(|_| anyhow::anyhow!("actor closed"))?;
        Ok(rx.await?)
    }

    pub async fn pr_open(&self, pr: MockPr) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_msg(ActorMsg::PrOpen { pr, response: tx }, rx)
            .await
    }

    pub async fn pr_close(&self, node_id: &str) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_msg(
            ActorMsg::PrClose {
                node_id: node_id.to_string(),
                response: tx,
            },
            rx,
        )
        .await
    }

    pub async fn add_review(
        &self,
        owner: &str,
        name: &str,
        pr_number: i64,
        reviewer: &str,
        state: &str,
    ) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_msg(
            ActorMsg::AddReview {
                owner: owner.to_string(),
                name: name.to_string(),
                pr_number,
                reviewer: reviewer.to_string(),
                state: state.to_string(),
                response: tx,
            },
            rx,
        )
        .await
    }

    pub async fn add_maintainer(&self, owner: &str, name: &str, login: &str) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_msg(
            ActorMsg::AddMaintainer {
                owner: owner.to_string(),
                name: name.to_string(),
                login: login.to_string(),
                response: tx,
            },
            rx,
        )
        .await
    }

    pub async fn add_ruleset(
        &self,
        owner: &str,
        name: &str,
        ruleset: MockRuleset,
    ) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_msg(
            ActorMsg::AddRuleset {
                owner: owner.to_string(),
                name: name.to_string(),
                ruleset,
                response: tx,
            },
            rx,
        )
        .await
    }

    pub async fn complete_check_run(
        &self,
        owner: &str,
        name: &str,
        check_name: &str,
        head_sha: &str,
        conclusion: &str,
    ) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_msg(
            ActorMsg::CompleteCheckRun {
                owner: owner.to_string(),
                name: name.to_string(),
                check_name: check_name.to_string(),
                head_sha: head_sha.to_string(),
                conclusion: conclusion.to_string(),
                response: tx,
            },
            rx,
        )
        .await
    }
}

async fn handle_git(
    State(tx): State<mpsc::UnboundedSender<ActorMsg>>,
    req: axum::extract::Request,
) -> Response<Body> {
    let path = req.uri().path().trim_start_matches('/');
    let segments: Vec<&str> = path.split('/').collect();

    if segments.len() < 2 {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("repository path must be /owner/name/..."))
            .expect("building error response");
    }

    let owner = segments[0].to_string();
    let name = segments[1].to_string();
    let remaining = if segments.len() > 2 {
        format!("/{}", segments[2..].join("/"))
    } else {
        String::new()
    };

    let (mut parts, body) = req.into_parts();
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let new_uri = format!("{}{}", remaining, query)
        .parse()
        .expect("building modified URI");
    parts.uri = new_uri;
    let modified_req = axum::extract::Request::from_parts(parts, body);

    let (resp_tx, resp_rx) = oneshot::channel();
    if tx
        .send(ActorMsg::ServeGitHttp {
            owner,
            name,
            request: modified_req,
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

async fn handle_graphql(
    State(tx): State<mpsc::UnboundedSender<ActorMsg>>,
    req: axum::extract::Request,
) -> Response<Body> {
    let (resp_tx, resp_rx) = oneshot::channel();
    if tx
        .send(ActorMsg::GraphQLRequest {
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
