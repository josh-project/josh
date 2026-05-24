use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use tokio::sync::{mpsc, oneshot};

use crate::graphql;
use crate::graphql::{MockPr, MockRuleset};

pub(crate) enum ActorMsg {
    ServeGitHttp {
        owner: String,
        name: String,
        request: axum::extract::Request,
        response: oneshot::Sender<Response<Body>>,
    },
    GraphQLRequest {
        request: axum::extract::Request,
        response: oneshot::Sender<Response<Body>>,
    },
    PrOpen {
        pr: MockPr,
        response: oneshot::Sender<()>,
    },
    PrClose {
        node_id: String,
        response: oneshot::Sender<()>,
    },
    AddReview {
        owner: String,
        name: String,
        pr_number: i64,
        reviewer: String,
        state: String,
        response: oneshot::Sender<()>,
    },
    AddMaintainer {
        owner: String,
        name: String,
        login: String,
        response: oneshot::Sender<()>,
    },
    AddRuleset {
        owner: String,
        name: String,
        ruleset: MockRuleset,
        response: oneshot::Sender<()>,
    },
    CompleteCheckRun {
        owner: String,
        name: String,
        check_name: String,
        head_sha: String,
        conclusion: String,
        response: oneshot::Sender<()>,
    },
}

pub(crate) async fn run_actor(
    mut rx: mpsc::UnboundedReceiver<ActorMsg>,
    repos: HashMap<(String, String), PathBuf>,
    state: Arc<Mutex<graphql::GraphQLState>>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            ActorMsg::ServeGitHttp {
                owner,
                name,
                request,
                response,
            } => {
                let key = (owner, name);
                let result = match repos.get(&key) {
                    Some(repo_path) => {
                        josh_cq_test_components::git_http::serve(repo_path, request).await
                    }
                    None => Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::from("repository not found"))
                        .expect("building error response"),
                };
                if response.send(result).is_err() {
                    tracing::error!("failed to send ServeGitHttp response");
                }
            }
            ActorMsg::GraphQLRequest { request, response } => {
                let result = graphql::handle_graphql_request(&repos, &state, request).await;
                if response.send(result).is_err() {
                    tracing::error!("failed to send GraphQLRequest response");
                }
            }
            ActorMsg::PrOpen { pr, response } => {
                let hook = {
                    let state_lock = state.lock().unwrap();
                    let hook = state_lock
                        .webhook_url
                        .as_ref()
                        .zip(state_lock.sim_url.as_ref())
                        .map(|(wh_url, sim_url)| {
                            (
                                wh_url.clone(),
                                graphql::webhooks::build_pr_opened_event(&pr, sim_url),
                            )
                        });
                    drop(state_lock);
                    if let Some(repo) = state.lock().unwrap().repo_mut(&pr.owner, &pr.name) {
                        repo.prs.push(pr);
                    }
                    hook
                };
                if let Some((wh_url, body)) = hook {
                    graphql::webhooks::send_webhook(&wh_url, "pull_request", body).await;
                }
                let _ = response.send(());
            }
            ActorMsg::PrClose { node_id, response } => {
                let hook = {
                    let state_lock = state.lock().unwrap();
                    state_lock
                        .webhook_url
                        .as_ref()
                        .zip(state_lock.sim_url.as_ref())
                        .and_then(|(wh_url, sim_url)| {
                            state_lock.find_pr_idx(&node_id).map(|(owner, name, idx)| {
                                let key = (owner.to_string(), name.to_string());
                                let pr = &state_lock.repos[&key].prs[idx];
                                (
                                    wh_url.clone(),
                                    graphql::webhooks::build_pr_closed_event(pr, sim_url),
                                    owner.to_string(),
                                    name.to_string(),
                                    idx,
                                )
                            })
                        })
                };
                if let Some((wh_url, body, owner, name, idx)) = hook {
                    {
                        let mut state_lock = state.lock().unwrap();
                        let key = (owner.clone(), name.clone());
                        if let Some(repo) = state_lock.repos.get_mut(&key) {
                            repo.closed_prs.push(node_id.clone());
                            if idx < repo.prs.len() && repo.prs[idx].node_id == node_id {
                                repo.prs.remove(idx);
                            }
                        }
                    }
                    graphql::webhooks::send_webhook(&wh_url, "pull_request", body).await;
                }
                let _ = response.send(());
            }
            ActorMsg::AddReview {
                owner,
                name,
                pr_number,
                reviewer,
                state: review_state,
                response,
            } => {
                let hook = {
                    let mut state_lock = state.lock().unwrap();
                    if let Some(repo) = state_lock.repo_mut(&owner, &name) {
                        repo.reviews
                            .entry(pr_number)
                            .or_default()
                            .push((reviewer.clone(), review_state.clone()));
                    }
                    let hook = state_lock
                        .webhook_url
                        .as_ref()
                        .zip(state_lock.sim_url.as_ref())
                        .and_then(|(wh_url, sim_url)| {
                            state_lock
                                .repo(&owner, &name)
                                .and_then(|repo| repo.prs.iter().find(|p| p.number == pr_number))
                                .map(|pr| {
                                    let clone_url = sim_url
                                        .join(&format!("{}/{}", owner, name))
                                        .map(|u| u.to_string())
                                        .unwrap_or_default();
                                    (
                                        wh_url.clone(),
                                        graphql::webhooks::build_pr_review_event(
                                            pr,
                                            &reviewer,
                                            &review_state,
                                            &clone_url,
                                        ),
                                    )
                                })
                        });
                    hook
                };
                if let Some((wh_url, body)) = hook {
                    graphql::webhooks::send_webhook(&wh_url, "pull_request_review", body).await;
                }
                let _ = response.send(());
            }
            ActorMsg::AddMaintainer {
                owner,
                name,
                login,
                response,
            } => {
                if let Some(repo) = state.lock().unwrap().repo_mut(&owner, &name) {
                    repo.maintainers.push(login);
                }
                let _ = response.send(());
            }
            ActorMsg::AddRuleset {
                owner,
                name,
                ruleset,
                response,
            } => {
                if let Some(repo) = state.lock().unwrap().repo_mut(&owner, &name) {
                    repo.rulesets.push(ruleset);
                }
                let _ = response.send(());
            }
            ActorMsg::CompleteCheckRun {
                owner,
                name,
                check_name,
                head_sha,
                conclusion,
                response,
            } => {
                let hook = {
                    let state_lock = state.lock().unwrap();
                    state_lock
                        .webhook_url
                        .as_ref()
                        .zip(state_lock.sim_url.as_ref())
                        .map(|(wh_url, sim_url)| {
                            let clone_url = sim_url
                                .join(&format!("{}/{}", owner, name))
                                .map(|u| u.to_string())
                                .unwrap_or_default();
                            (
                                wh_url.clone(),
                                graphql::webhooks::build_check_run_event(
                                    &check_name,
                                    &head_sha,
                                    &conclusion,
                                    &clone_url,
                                ),
                            )
                        })
                };
                if let Some((wh_url, body)) = hook {
                    graphql::webhooks::send_webhook(&wh_url, "check_run", body).await;
                }
                let _ = response.send(());
            }
        }
    }
}
