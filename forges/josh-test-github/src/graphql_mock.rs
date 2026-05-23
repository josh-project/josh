use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use url::Url;

// Operation names — must match the operation names in the .graphql query files
// in josh-github-codegen-graphql.
mod op {
    pub const GET_OPEN_PRS: &str = "GetOpenPrs";
    pub const GET_PR_REVIEWS: &str = "GetPrReviews";
    pub const GET_PRS_BY_SHA: &str = "GetPrsBySha";
    pub const GET_REPOSITORY_COLLABORATORS: &str = "GetRepositoryCollaborators";
    pub const GET_REPOSITORY_RULESETS: &str = "GetRepositoryRulesets";
    pub const GET_RULESET_REQUIRED_CHECKS: &str = "GetRulesetRequiredChecks";
    pub const CLOSE_PULL_REQUEST: &str = "ClosePullRequest";
    pub const ADD_PR_COMMENT: &str = "AddPrComment";
}

pub struct GraphQLMock {
    data: Arc<Mutex<GraphQLData>>,
}

#[derive(Default)]
pub struct GraphQLData {
    pub prs: Vec<MockPr>,
    pub reviews: BTreeMap<i64, Vec<(String, String)>>, // pr_number → [(login, state)]
    pub maintainers: Vec<String>,
    pub rulesets: Vec<MockRuleset>,
    pub closed_prs: Vec<String>,
    pub comments: Vec<(String, String)>, // (subject_id, body)
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
    pub required_checks: Vec<String>,
}

impl GraphQLData {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn with_pr(&mut self, pr: MockPr) -> &mut Self {
        self.prs.push(pr);
        self
    }

    pub fn with_review(&mut self, pr_number: i64, login: &str, state: &str) -> &mut Self {
        self.reviews
            .entry(pr_number)
            .or_default()
            .push((login.to_string(), state.to_string()));
        self
    }

    pub fn with_maintainer(&mut self, login: &str) -> &mut Self {
        self.maintainers.push(login.to_string());
        self
    }

    pub fn with_ruleset(&mut self, ruleset: MockRuleset) -> &mut Self {
        self.rulesets.push(ruleset);
        self
    }
}

impl GraphQLMock {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(GraphQLData::default())),
        }
    }

    pub fn from_data(data: GraphQLData) -> Self {
        Self {
            data: Arc::new(Mutex::new(data)),
        }
    }

    pub async fn serve(&self) -> anyhow::Result<(tokio::task::JoinHandle<()>, Url)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let url = Url::parse(&format!("http://{}/graphql", addr))?;

        let state = self.data.clone();

        let app = Router::new()
            .route("/graphql", post(handle_graphql))
            .with_state(state);

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Ok((handle, url))
    }

    pub fn closed_pr_node_ids(&self) -> Vec<String> {
        self.data.lock().unwrap().closed_prs.clone()
    }

    pub fn comments(&self) -> Vec<(String, String)> {
        self.data.lock().unwrap().comments.clone()
    }
}

async fn handle_graphql(
    State(data): State<Arc<Mutex<GraphQLData>>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let operation_name = body["operationName"].as_str().unwrap_or("");

    // Lock once per request — hold time is microseconds, no await in between.
    let mut inner = data.lock().unwrap();

    let response = match operation_name {
        op::GET_OPEN_PRS => handle_get_open_prs(&mut inner, &body),
        op::GET_PR_REVIEWS => handle_get_pr_reviews(&inner, &body),
        op::GET_PRS_BY_SHA => handle_get_prs_by_sha(&inner, &body),
        op::GET_REPOSITORY_COLLABORATORS => handle_get_repository_collaborators(&inner),
        op::GET_REPOSITORY_RULESETS => handle_get_repository_rulesets(&inner),
        op::GET_RULESET_REQUIRED_CHECKS => handle_get_ruleset_required_checks(&mut inner, &body),
        op::CLOSE_PULL_REQUEST => handle_close_pull_request(&mut inner, &body),
        op::ADD_PR_COMMENT => handle_add_pr_comment(&mut inner, &body),
        _ => serde_json::json!({
            "errors": [{"message": format!("Unknown operation: {}", operation_name)}]
        }),
    };

    (StatusCode::OK, Json(response)).into_response()
}

fn handle_get_open_prs(inner: &mut GraphQLData, body: &serde_json::Value) -> serde_json::Value {
    let first = body["variables"]["first"].as_i64().unwrap_or(100).min(100) as usize;
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

fn handle_get_pr_reviews(inner: &GraphQLData, body: &serde_json::Value) -> serde_json::Value {
    let pr_number = body["variables"]["number"].as_i64().unwrap_or(0);
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

fn handle_get_prs_by_sha(inner: &GraphQLData, body: &serde_json::Value) -> serde_json::Value {
    let sha = body["variables"]["sha"].as_str().unwrap_or("");
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

fn handle_get_repository_collaborators(inner: &GraphQLData) -> serde_json::Value {
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

fn handle_get_repository_rulesets(inner: &GraphQLData) -> serde_json::Value {
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
    inner: &mut GraphQLData,
    body: &serde_json::Value,
) -> serde_json::Value {
    let ruleset_id = body["variables"]["rulesetId"].as_str().unwrap_or("");
    let ruleset_idx = inner.rulesets.iter().position(|rs| rs.id == ruleset_id);

    let (ruleset_name, checks) = match ruleset_idx {
        Some(idx) => {
            let rs = &inner.rulesets[idx];
            let required_status_checks: Vec<serde_json::Value> = rs
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

            let rs = inner.rulesets.remove(idx);
            (rs.name, rules_nodes)
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

fn handle_close_pull_request(
    inner: &mut GraphQLData,
    body: &serde_json::Value,
) -> serde_json::Value {
    let node_id = body["variables"]["pullRequestNodeId"]
        .as_str()
        .unwrap_or("")
        .to_string();
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

fn handle_add_pr_comment(inner: &mut GraphQLData, body: &serde_json::Value) -> serde_json::Value {
    let subject_id = body["variables"]["subjectId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let comment_body = body["variables"]["body"].as_str().unwrap_or("").to_string();
    inner.comments.push((subject_id.clone(), comment_body));

    serde_json::json!({
        "data": {
            "addComment": {
                "clientMutationId": null
            }
        }
    })
}
