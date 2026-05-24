use std::collections::HashMap;
use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::FromRequest;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use indexmap::IndexMap;
use juniper::{
    DefaultScalarValue, EmptyMutation, EmptySubscription, InputValue, Variables, graphql_object,
};
use serde::Deserialize;

struct Context {
    repos: HashMap<(String, String), PathBuf>,
}

impl juniper::Context for Context {}

struct Query;

#[graphql_object(context = Context)]
impl Query {
    async fn repository(owner: String, name: String, context: &Context) -> Option<Repository> {
        let key = (owner.clone(), name.clone());
        if context.repos.contains_key(&key) {
            Some(Repository { owner, name })
        } else {
            None
        }
    }
}

struct Repository {
    owner: String,
    name: String,
}

#[graphql_object(context = Context)]
impl Repository {
    fn name_with_owner(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    fn default_branch_ref(&self) -> DefaultBranchRef {
        DefaultBranchRef
    }
}

struct DefaultBranchRef;

#[graphql_object(context = Context)]
impl DefaultBranchRef {
    fn name(&self) -> &str {
        "main"
    }
}

type Schema = juniper::RootNode<Query, EmptyMutation<Context>, EmptySubscription<Context>>;

fn create_schema() -> Schema {
    Schema::new(Query, EmptyMutation::new(), EmptySubscription::new())
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

pub(crate) async fn handle_graphql_request(
    repos: &HashMap<(String, String), PathBuf>,
    request: axum::extract::Request,
) -> Response<Body> {
    let Json(payload) = match Json::<GraphQLPayload>::from_request(request, &()).await {
        Ok(p) => p,
        Err(e) => return GraphQLError::from_message(e.to_string()).into_response(),
    };

    let variables = variables_to_juniper(&payload.variables.unwrap_or_default());

    let context = Context {
        repos: repos.clone(),
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

    (
        StatusCode::OK,
        Json(juniper::http::GraphQLResponse::from_result(result)),
    )
        .into_response()
}
