use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::body::Body;
use axum::extract::FromRequest;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use indexmap::IndexMap;
use juniper::{DefaultScalarValue, EmptySubscription, InputValue, Variables};
use serde::Deserialize;
use url::Url;

mod collaborator;
mod context;
mod git_object;
mod mutation;
mod pull_request;
mod query;
mod repository;
mod ruleset;
mod types;
pub(crate) mod webhooks;

pub use types::{GraphQLState, MockPr, MockRuleset, RepoState, ReviewState};

use context::Context;
use mutation::Mutation;
use query::Query;

type Schema = juniper::RootNode<Query, Mutation, EmptySubscription<Context>>;

fn create_schema() -> Schema {
    Schema::new(Query, Mutation, EmptySubscription::new())
}

#[derive(Deserialize)]
struct GraphQLPayload {
    query: String,
    #[serde(rename = "operationName")]
    operation_name: Option<String>,
    variables: Option<serde_json::Value>,
}

fn variables_to_juniper(json: &serde_json::Value) -> Variables<DefaultScalarValue> {
    let mut vars = Variables::new();
    if let Some(obj) = json.as_object() {
        for (key, value) in obj.iter() {
            vars.insert(key.clone(), to_input_value(value));
        }
    }
    vars
}

fn to_input_value(json: &serde_json::Value) -> InputValue<DefaultScalarValue> {
    match json {
        serde_json::Value::Null => InputValue::Null,
        serde_json::Value::Bool(b) => InputValue::scalar(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                InputValue::scalar(i as i32)
            } else if let Some(f) = n.as_f64() {
                InputValue::scalar(f)
            } else {
                InputValue::Null
            }
        }
        serde_json::Value::String(s) => InputValue::scalar(s.clone()),
        serde_json::Value::Array(arr) => InputValue::list(arr.iter().map(to_input_value).collect()),
        serde_json::Value::Object(obj) => {
            let map: IndexMap<String, InputValue<DefaultScalarValue>> = obj
                .iter()
                .map(|(k, v)| (k.clone(), to_input_value(v)))
                .collect();
            InputValue::object(map)
        }
    }
}

struct GraphQLError {
    body: serde_json::Value,
}

impl GraphQLError {
    fn from_message(message: impl Into<String>) -> Self {
        Self {
            body: serde_json::json!({"errors": [{"message": message.into()}]}),
        }
    }
}

impl IntoResponse for GraphQLError {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self.body)).into_response()
    }
}

fn close_pr_webhook_data(
    payload: &GraphQLPayload,
    state: &Arc<Mutex<GraphQLState>>,
) -> Option<(Url, serde_json::Value)> {
    let state_lock = state.lock().unwrap();
    let webhook_url = state_lock.webhook_url.clone();
    match (
        webhook_url,
        payload.operation_name.as_deref(),
        state_lock.sim_url.as_ref(),
    ) {
        (Some(wh_url), Some("ClosePullRequest"), Some(sim_url)) => {
            let pr_id = payload
                .variables
                .as_ref()
                .and_then(|v| v.get("pullRequestNodeId"))
                .and_then(|v| v.as_str());
            pr_id
                .and_then(|id| {
                    state_lock.find_pr_idx(id).map(|(owner, name, idx)| {
                        let key = (owner.to_string(), name.to_string());
                        (owner, name, &state_lock.repos[&key].prs[idx])
                    })
                })
                .map(|(owner, name, pr)| {
                    (
                        wh_url,
                        webhooks::build_pr_closed_event(&owner, &name, pr, sim_url),
                    )
                })
        }
        _ => None,
    }
}

pub(crate) async fn handle_graphql_request(
    repos: &HashMap<(String, String), PathBuf>,
    state: &Arc<Mutex<GraphQLState>>,
    request: axum::extract::Request,
) -> Response<Body> {
    let Json(payload) = match Json::<GraphQLPayload>::from_request(request, &()).await {
        Ok(p) => p,
        Err(e) => return GraphQLError::from_message(e.to_string()).into_response(),
    };

    let variables = variables_to_juniper(&payload.variables.clone().unwrap_or_default());

    // Capture PR data before mutation executes — the mutation removes it from state.
    let webhook_data = close_pr_webhook_data(&payload, state);

    let context = Context {
        repos: repos.clone(),
        state: state.clone(),
    };
    let schema = create_schema();

    let result = juniper::execute(
        &payload.query,
        payload.operation_name.as_deref(),
        &schema,
        &variables,
        &context,
    )
    .await;

    if let Some((wh_url, body)) = webhook_data {
        webhooks::send_webhook(&wh_url, "pull_request", body).await;
    }

    (
        StatusCode::OK,
        Json(juniper::http::GraphQLResponse::from_result(result)),
    )
        .into_response()
}
