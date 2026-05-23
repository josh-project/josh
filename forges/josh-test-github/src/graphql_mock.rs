use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use url::Url;

pub struct GraphQLMock {
    state: Arc<GraphQLState>,
}

struct GraphQLState {
    inner: Mutex<GraphQLStateInner>,
}

struct GraphQLStateInner {
    prs: Vec<MockPr>,
    reviews: BTreeMap<i64, Vec<(String, String)>>, // pr_number → [(login, state)]
    maintainers: Vec<String>,
    rulesets: Vec<MockRuleset>,
    required_checks: Vec<String>,
    closed_prs: Vec<String>,
    comments: Vec<(String, String)>, // (subject_id, body)
}

pub struct MockPr {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub head_ref_name: String,
    pub head_ref_oid: String,
    pub base_ref_name: String,
    pub base_ref_oid: String,
}

pub struct MockRuleset {
    pub id: String,
    pub name: String,
    pub enforcement: String,
    pub include_refs: Vec<String>,
    pub exclude_refs: Vec<String>,
}

impl GraphQLMock {
    pub fn new() -> Self {
        Self {
            state: Arc::new(GraphQLState {
                inner: Mutex::new(GraphQLStateInner {
                    prs: Vec::new(),
                    reviews: BTreeMap::new(),
                    maintainers: Vec::new(),
                    rulesets: Vec::new(),
                    required_checks: Vec::new(),
                    closed_prs: Vec::new(),
                    comments: Vec::new(),
                }),
            }),
        }
    }

    pub fn with_pr(self, pr: MockPr) -> Self {
        self.state.inner.lock().unwrap().prs.push(pr);
        self
    }

    pub fn with_review(self, pr_number: i64, login: &str, state: &str) -> Self {
        self.state
            .inner
            .lock()
            .unwrap()
            .reviews
            .entry(pr_number)
            .or_default()
            .push((login.to_string(), state.to_string()));
        self
    }

    pub fn with_maintainer(self, login: &str) -> Self {
        self.state
            .inner
            .lock()
            .unwrap()
            .maintainers
            .push(login.to_string());
        self
    }

    pub fn with_ruleset(self, ruleset: MockRuleset) -> Self {
        self.state.inner.lock().unwrap().rulesets.push(ruleset);
        self
    }

    pub fn with_required_check(self, context: &str) -> Self {
        self.state
            .inner
            .lock()
            .unwrap()
            .required_checks
            .push(context.to_string());
        self
    }

    pub async fn serve(&self) -> anyhow::Result<(tokio::task::JoinHandle<()>, Url)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let url = Url::parse(&format!("http://{}/graphql", addr))?;

        let state = self.state.clone();

        let app = Router::new()
            .route("/graphql", post(handle_graphql))
            .with_state(state);

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Ok((handle, url))
    }

    pub fn closed_pr_node_ids(&self) -> Vec<String> {
        self.state.inner.lock().unwrap().closed_prs.clone()
    }

    pub fn comments(&self) -> Vec<(String, String)> {
        self.state.inner.lock().unwrap().comments.clone()
    }
}

async fn handle_graphql(
    State(state): State<Arc<GraphQLState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let operation_name = body["operationName"].as_str().unwrap_or("");

    let response = match operation_name {
        "GetOpenPrs" => handle_get_open_prs(&state, &body),
        "GetPrReviews" => handle_get_pr_reviews(&state, &body),
        "GetPrsBySha" => handle_get_prs_by_sha(&state, &body),
        "GetRepositoryCollaborators" => handle_get_repository_collaborators(&state),
        "GetRepositoryRulesets" => handle_get_repository_rulesets(&state),
        "GetRulesetRequiredChecks" => handle_get_ruleset_required_checks(&state, &body),
        "ClosePullRequest" => handle_close_pull_request(&state, &body),
        "AddPrComment" => handle_add_pr_comment(&state, &body),
        _ => serde_json::json!({
            "errors": [{"message": format!("Unknown operation: {}", operation_name)}]
        }),
    };

    (StatusCode::OK, Json(response)).into_response()
}

fn handle_get_open_prs(state: &GraphQLState, body: &serde_json::Value) -> serde_json::Value {
    let first = body["variables"]["first"].as_i64().unwrap_or(100).min(100) as usize;
    let inner = state.inner.lock().unwrap();
    let total_count = inner.prs.len() as i64;
    let nodes: Vec<serde_json::Value> = inner
        .prs
        .iter()
        .take(first)
        .map(|pr| {
            serde_json::json!({
                "id": pr.node_id,
                "number": pr.number,
                "title": pr.title,
                "headRefOid": pr.head_ref_oid,
                "headRefName": pr.head_ref_name,
                "baseRefOid": pr.base_ref_oid,
                "baseRefName": pr.base_ref_name,
            })
        })
        .collect();

    serde_json::json!({
        "data": {
            "repository": {
                "pullRequests": {
                    "nodes": nodes,
                    "totalCount": total_count,
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }
    })
}

fn handle_get_pr_reviews(state: &GraphQLState, body: &serde_json::Value) -> serde_json::Value {
    let pr_number = body["variables"]["number"].as_i64().unwrap_or(0);
    let inner = state.inner.lock().unwrap();
    let nodes: Vec<serde_json::Value> = inner
        .reviews
        .get(&pr_number)
        .map(|review_list| {
            review_list
                .iter()
                .map(|(login, review_state)| {
                    serde_json::json!({
                        "author": {
                            "__typename": "User",
                            "login": login,
                        },
                        "state": review_state,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    serde_json::json!({
        "data": {
            "repository": {
                "pullRequest": {
                    "reviews": {
                        "nodes": nodes,
                        "pageInfo": {
                            "hasNextPage": false,
                            "endCursor": null
                        }
                    }
                }
            }
        }
    })
}

fn handle_get_prs_by_sha(state: &GraphQLState, body: &serde_json::Value) -> serde_json::Value {
    let sha = body["variables"]["sha"].as_str().unwrap_or("");
    let inner = state.inner.lock().unwrap();
    let nodes: Vec<serde_json::Value> = inner
        .prs
        .iter()
        .filter(|pr| pr.head_ref_oid == sha)
        .map(|pr| {
            serde_json::json!({
                "id": pr.node_id,
                "number": pr.number,
            })
        })
        .collect();

    serde_json::json!({
        "data": {
            "repository": {
                "object": {
                    "__typename": "Commit",
                    "associatedPullRequests": {
                        "nodes": nodes
                    }
                }
            }
        }
    })
}

fn handle_get_repository_collaborators(state: &GraphQLState) -> serde_json::Value {
    let inner = state.inner.lock().unwrap();
    let edges: Vec<serde_json::Value> = inner
        .maintainers
        .iter()
        .map(|login| {
            serde_json::json!({
                "permission": "WRITE",
                "node": {
                    "login": login,
                }
            })
        })
        .collect();

    serde_json::json!({
        "data": {
            "repository": {
                "collaborators": {
                    "pageInfo": {
                        "endCursor": null,
                        "hasNextPage": false
                    },
                    "edges": edges
                }
            }
        }
    })
}

fn handle_get_repository_rulesets(state: &GraphQLState) -> serde_json::Value {
    let inner = state.inner.lock().unwrap();
    let nodes: Vec<serde_json::Value> = inner
        .rulesets
        .iter()
        .map(|rs| {
            serde_json::json!({
                "id": rs.id,
                "name": rs.name,
                "enforcement": rs.enforcement,
                "target": "BRANCH",
                "conditions": {
                    "refName": {
                        "include": rs.include_refs,
                        "exclude": rs.exclude_refs,
                    }
                }
            })
        })
        .collect();

    serde_json::json!({
        "data": {
            "repository": {
                "rulesets": {
                    "nodes": nodes
                }
            }
        }
    })
}

fn handle_get_ruleset_required_checks(
    state: &GraphQLState,
    body: &serde_json::Value,
) -> serde_json::Value {
    let ruleset_id = body["variables"]["rulesetId"].as_str().unwrap_or("");
    let inner = state.inner.lock().unwrap();
    let ruleset = inner.rulesets.iter().find(|rs| rs.id == ruleset_id);

    let (ruleset_name, checks) = match ruleset {
        Some(rs) => {
            let required_status_checks: Vec<serde_json::Value> = inner
                .required_checks
                .iter()
                .map(|context| {
                    serde_json::json!({
                        "context": context,
                        "integrationId": null,
                    })
                })
                .collect();

            let rules_nodes = if required_status_checks.is_empty() {
                vec![]
            } else {
                vec![serde_json::json!({
                    "type": "REQUIRED_STATUS_CHECKS",
                    "parameters": {
                        "__typename": "RequiredStatusChecksParameters",
                        "requiredStatusChecks": required_status_checks,
                        "strictRequiredStatusChecksPolicy": false,
                    }
                })]
            };

            (rs.name.clone(), rules_nodes)
        }
        None => (String::new(), vec![]),
    };

    serde_json::json!({
        "data": {
            "node": {
                "__typename": "RepositoryRuleset",
                "id": ruleset_id,
                "name": ruleset_name,
                "rules": {
                    "nodes": checks
                }
            }
        }
    })
}

fn handle_close_pull_request(state: &GraphQLState, body: &serde_json::Value) -> serde_json::Value {
    let node_id = body["variables"]["pullRequestNodeId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let mut inner = state.inner.lock().unwrap();
    inner.closed_prs.push(node_id.clone());
    // Remove from open PRs so subsequent GetOpenPrs queries don't re-discover it
    inner.prs.retain(|pr| pr.node_id != node_id);

    serde_json::json!({
        "data": {
            "closePullRequest": {
                "pullRequest": {
                    "id": node_id
                }
            }
        }
    })
}

fn handle_add_pr_comment(state: &GraphQLState, body: &serde_json::Value) -> serde_json::Value {
    let subject_id = body["variables"]["subjectId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let comment_body = body["variables"]["body"].as_str().unwrap_or("").to_string();
    state
        .inner
        .lock()
        .unwrap()
        .comments
        .push((subject_id.clone(), comment_body));

    serde_json::json!({
        "data": {
            "addComment": {
                "clientMutationId": null
            }
        }
    })
}
