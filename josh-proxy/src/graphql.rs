use std::{error::Error, fmt, sync::Arc};

use juniper::{
    GraphQLSubscriptionType, GraphQLType, GraphQLTypeAsync, InputValue, RootNode, ScalarValue,
    http::{GraphQLBatchRequest, GraphQLRequest as JuniperGraphQLRequest, GraphQLRequest},
};
use serde_json::error::Error as SerdeError;
use url::form_urlencoded;

use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use josh_core::JoshResult;

pub async fn graphql_sync<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    method: Method,
    content_type: Option<axum_extra::headers::Mime>,
    query: Option<String>,
    body: String,
) -> JoshResult<Response>
where
    QueryT: GraphQLType<S, Context = CtxT>,
    QueryT::TypeInfo: Sync,
    MutationT: GraphQLType<S, Context = CtxT>,
    MutationT::TypeInfo: Sync,
    SubscriptionT: GraphQLType<S, Context = CtxT>,
    SubscriptionT::TypeInfo: Sync,
    CtxT: Sync,
    S: ScalarValue + Send + Sync,
{
    Ok(match parse_req(method, content_type, query, body).await {
        Ok(req) => execute_request_sync(root_node, context, req).await,
        Err(resp) => resp,
    })
}

pub async fn graphql<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    method: Method,
    content_type: Option<axum_extra::headers::Mime>,
    query: Option<String>,
    body: String,
) -> JoshResult<Response>
where
    QueryT: GraphQLTypeAsync<S, Context = CtxT>,
    QueryT::TypeInfo: Sync,
    MutationT: GraphQLTypeAsync<S, Context = CtxT>,
    MutationT::TypeInfo: Sync,
    SubscriptionT: GraphQLSubscriptionType<S, Context = CtxT>,
    SubscriptionT::TypeInfo: Sync,
    CtxT: Sync,
    S: ScalarValue + Send + Sync,
{
    Ok(match parse_req(method, content_type, query, body).await {
        Ok(req) => execute_request(root_node, context, req).await,
        Err(resp) => resp,
    })
}

pub async fn parse_req<S: ScalarValue>(
    method: Method,
    content_type: Option<axum_extra::headers::Mime>,
    query: Option<String>,
    body: String,
) -> Result<GraphQLBatchRequest<S>, Response> {
    match method {
        Method::GET => parse_get_req(query),
        Method::POST => {
            let content_type = content_type
                .as_ref()
                .map(|ct| (ct.type_(), ct.subtype().as_str()));

            match content_type {
                Some((mime::APPLICATION, "json")) => parse_post_json_req(body).await,
                Some((mime::APPLICATION, "graphql")) => parse_post_graphql_req(body).await,
                _ => return Err(StatusCode::BAD_REQUEST.into_response()),
            }
        }
        _ => return Err(StatusCode::METHOD_NOT_ALLOWED.into_response()),
    }
    .map_err(render_error)
}

fn parse_get_req<S: ScalarValue>(
    query_string: Option<String>,
) -> Result<GraphQLBatchRequest<S>, GraphQLRequestError> {
    query_string
        .map(|q| gql_request_from_get(&q).map(GraphQLBatchRequest::Single))
        .unwrap_or_else(|| {
            Err(GraphQLRequestError::Invalid(
                "'query' parameter is missing".to_string(),
            ))
        })
}

async fn parse_post_json_req<S: ScalarValue>(
    body: String,
) -> Result<GraphQLBatchRequest<S>, GraphQLRequestError> {
    serde_json::from_str::<GraphQLBatchRequest<S>>(&body)
        .map_err(GraphQLRequestError::BodyJSONError)
}

async fn parse_post_graphql_req<S: ScalarValue>(
    body: String,
) -> Result<GraphQLBatchRequest<S>, GraphQLRequestError> {
    Ok(GraphQLBatchRequest::Single(GraphQLRequest::new(
        body, None, None,
    )))
}

pub fn graphiql(graphql_endpoint: &str, subscriptions_endpoint: Option<&str>) -> Response {
    let html = juniper::http::graphiql::graphiql_source(graphql_endpoint, subscriptions_endpoint);
    axum::response::Html(html).into_response()
}

pub async fn playground(graphql_endpoint: &str, subscriptions_endpoint: Option<&str>) -> Response {
    let html =
        juniper::http::playground::playground_source(graphql_endpoint, subscriptions_endpoint);
    axum::response::Html(html).into_response()
}

fn render_error(e: GraphQLRequestError) -> Response {
    (StatusCode::BAD_REQUEST, e.to_string()).into_response()
}

async fn execute_request_sync<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    request: GraphQLBatchRequest<S>,
) -> Response
where
    QueryT: GraphQLType<S, Context = CtxT>,
    QueryT::TypeInfo: Sync,
    MutationT: GraphQLType<S, Context = CtxT>,
    MutationT::TypeInfo: Sync,
    SubscriptionT: GraphQLType<S, Context = CtxT>,
    SubscriptionT::TypeInfo: Sync,
    CtxT: Sync,
    S: ScalarValue + Send + Sync,
{
    let res = request.execute_sync(&*root_node, &context);
    let code = if res.is_ok() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (code, axum::response::Json(res)).into_response()
}

pub async fn execute_request<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    request: GraphQLBatchRequest<S>,
) -> Response
where
    QueryT: GraphQLTypeAsync<S, Context = CtxT>,
    QueryT::TypeInfo: Sync,
    MutationT: GraphQLTypeAsync<S, Context = CtxT>,
    MutationT::TypeInfo: Sync,
    SubscriptionT: GraphQLSubscriptionType<S, Context = CtxT>,
    SubscriptionT::TypeInfo: Sync,
    CtxT: Sync,
    S: ScalarValue + Send + Sync,
{
    let res = request.execute(&*root_node, &context).await;
    let code = if res.is_ok() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (code, axum::response::Json(&res)).into_response()
}

fn gql_request_from_get<S>(input: &str) -> Result<JuniperGraphQLRequest<S>, GraphQLRequestError>
where
    S: ScalarValue,
{
    let mut query = None;
    let operation_name = None;
    let mut variables = None;
    for (key, value) in form_urlencoded::parse(input.as_bytes()).into_owned() {
        match key.as_ref() {
            "query" => {
                if query.is_some() {
                    return Err(invalid_err("query"));
                }
                query = Some(value)
            }
            "operationName" => {
                if operation_name.is_some() {
                    return Err(invalid_err("operationName"));
                }
            }
            "variables" => {
                if variables.is_some() {
                    return Err(invalid_err("variables"));
                }
                match serde_json::from_str::<InputValue<S>>(&value)
                    .map_err(GraphQLRequestError::Variables)
                {
                    Ok(parsed_variables) => variables = Some(parsed_variables),
                    Err(e) => return Err(e),
                }
            }
            _ => continue,
        }
    }
    match query {
        Some(query) => Ok(JuniperGraphQLRequest::new(query, operation_name, variables)),
        None => Err(GraphQLRequestError::Invalid(
            "'query' parameter is missing".to_string(),
        )),
    }
}

fn invalid_err(parameter_name: &str) -> GraphQLRequestError {
    GraphQLRequestError::Invalid(format!(
        "'{}' parameter is specified multiple times",
        parameter_name
    ))
}

#[derive(Debug)]
enum GraphQLRequestError {
    BodyJSONError(SerdeError),
    Variables(SerdeError),
    Invalid(String),
}

impl fmt::Display for GraphQLRequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            GraphQLRequestError::BodyJSONError(ref err) => fmt::Display::fmt(err, f),
            GraphQLRequestError::Variables(ref err) => fmt::Display::fmt(err, f),
            GraphQLRequestError::Invalid(ref err) => fmt::Display::fmt(err, f),
        }
    }
}

impl Error for GraphQLRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            GraphQLRequestError::BodyJSONError(ref err) => Some(err),
            GraphQLRequestError::Variables(ref err) => Some(err),
            GraphQLRequestError::Invalid(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{Router, extract::State, routing::post};
    use axum_extra::TypedHeader;

    use reqwest::blocking::Response as ReqwestResponse;
    use std::{net::SocketAddr, sync::Arc, thread, time::Duration};
    use tokio::net::TcpListener;

    use juniper::{
        EmptyMutation, EmptySubscription, RootNode,
        http::tests as http_tests,
        tests::fixtures::starwars::schema::{Database, Query},
    };

    #[derive(Clone)]
    struct AppState {
        root_node: Arc<RootNode<Query, EmptyMutation<Database>, EmptySubscription<Database>>>,
        db: Arc<Database>,
    }

    struct TestAxumIntegration {
        port: u16,
    }

    impl http_tests::HttpIntegration for TestAxumIntegration {
        fn get(&self, url: &str) -> http_tests::TestResponse {
            let url = format!("http://127.0.0.1:{}/graphql{}", self.port, url);
            make_test_response(
                reqwest::blocking::get(&url).unwrap_or_else(|_| panic!("failed GET {}", url)),
            )
        }

        fn post_json(&self, url: &str, body: &str) -> http_tests::TestResponse {
            let url = format!("http://127.0.0.1:{}/graphql{}", self.port, url);
            let client = reqwest::blocking::Client::new();
            let res = client
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string())
                .send()
                .unwrap_or_else(|_| panic!("failed POST {}", url));
            make_test_response(res)
        }

        fn post_graphql(&self, url: &str, body: &str) -> http_tests::TestResponse {
            let url = format!("http://127.0.0.1:{}/graphql{}", self.port, url);
            let client = reqwest::blocking::Client::new();
            let res = client
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/graphql")
                .body(body.to_string())
                .send()
                .unwrap_or_else(|_| panic!("failed POST {}", url));
            make_test_response(res)
        }
    }

    fn make_test_response(response: ReqwestResponse) -> http_tests::TestResponse {
        let status_code = response.status().as_u16() as i32;
        let content_type_header = response.headers().get(reqwest::header::CONTENT_TYPE);
        let content_type = if let Some(ct) = content_type_header {
            ct.to_str().unwrap().to_string()
        } else {
            String::default()
        };
        let body = response.text().unwrap();

        http_tests::TestResponse {
            status_code,
            body: Some(body),
            content_type,
        }
    }

    async fn graphql_handler(
        State(state): State<AppState>,
        method: axum::http::Method,
        content_type: Option<TypedHeader<axum_extra::headers::ContentType>>,
        axum::extract::RawQuery(query): axum::extract::RawQuery,
        body: String,
    ) -> axum::response::Result<axum::response::Response> {
        let content_type_header = content_type.map(|TypedHeader(ct)| ct.into());

        super::graphql(
            state.root_node,
            state.db,
            method,
            content_type_header,
            query,
            body,
        )
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{:?}", e),
            )
                .into()
        })
    }

    async fn graphql_sync_handler(
        State(state): State<AppState>,
        method: axum::http::Method,
        content_type: Option<TypedHeader<axum_extra::headers::ContentType>>,
        axum::extract::RawQuery(query): axum::extract::RawQuery,
        body: String,
    ) -> axum::response::Result<axum::response::Response> {
        let content_type_header = content_type.map(|TypedHeader(ct)| ct.into());

        super::graphql_sync(
            state.root_node,
            state.db,
            method,
            content_type_header,
            query,
            body,
        )
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{:?}", e),
            )
                .into()
        })
    }

    async fn run_axum_integration(is_sync: bool) {
        let port = if is_sync { 3002 } else { 3001 };
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();

        let db = Arc::new(Database::new());
        let root_node = Arc::new(RootNode::new(
            Query,
            EmptyMutation::<Database>::new(),
            EmptySubscription::<Database>::new(),
        ));

        let state = AppState { root_node, db };

        let handler = if is_sync {
            post(graphql_sync_handler).get(graphql_sync_handler)
        } else {
            post(graphql_handler).get(graphql_handler)
        };

        let app = Router::new()
            .route("/graphql", handler.clone())
            .route("/graphql/", handler)
            .with_state(state);

        let listener = TcpListener::bind(addr).await.unwrap();
        println!("Listening on http://{}", addr);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::task::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_rx.await.ok();
                })
                .await
                .unwrap();
        });

        tokio::task::spawn_blocking(move || {
            thread::sleep(Duration::from_millis(10)); // wait 10ms for server to bind
            let integration = TestAxumIntegration { port };
            http_tests::run_http_test_suite(&integration);
            shutdown_tx.send(()).ok();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_axum_integration() {
        run_axum_integration(false).await
    }

    #[tokio::test]
    async fn test_sync_axum_integration() {
        run_axum_integration(true).await
    }
}
