use std::{error::Error, fmt, string::FromUtf8Error, sync::Arc};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    Method, Request, Response, StatusCode,
    body::Incoming,
    header::{self, HeaderValue},
};
use juniper::{
    GraphQLSubscriptionType, GraphQLType, GraphQLTypeAsync, InputValue, RootNode, ScalarValue,
    http::{GraphQLBatchRequest, GraphQLRequest as JuniperGraphQLRequest, GraphQLRequest},
};
use serde_json::error::Error as SerdeError;
use url::form_urlencoded;

pub async fn graphql_sync<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<'static, QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error>
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
    Ok(match parse_req(req).await {
        Ok(req) => execute_request_sync(root_node, context, req).await,
        Err(resp) => resp,
    })
}

pub async fn graphql<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<'static, QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error>
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
    Ok(match parse_req(req).await {
        Ok(req) => execute_request(root_node, context, req).await,
        Err(resp) => resp,
    })
}

pub async fn parse_req<S: ScalarValue>(
    req: Request<Incoming>,
) -> Result<GraphQLBatchRequest<S>, Response<Full<Bytes>>> {
    match *req.method() {
        Method::GET => parse_get_req(req),
        Method::POST => {
            let content_type = req
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|x| HeaderValue::to_str(x).ok())
                .and_then(|x| x.split(';').next());
            match content_type {
                Some("application/json") => parse_post_json_req(req.into_body()).await,
                Some("application/graphql") => parse_post_graphql_req(req.into_body()).await,
                _ => return Err(new_response(StatusCode::BAD_REQUEST)),
            }
        }
        _ => return Err(new_response(StatusCode::METHOD_NOT_ALLOWED)),
    }
    .map_err(render_error)
}

fn parse_get_req<S: ScalarValue>(
    req: Request<Incoming>,
) -> Result<GraphQLBatchRequest<S>, GraphQLRequestError> {
    req.uri()
        .query()
        .map(|q| gql_request_from_get(q).map(GraphQLBatchRequest::Single))
        .unwrap_or_else(|| {
            Err(GraphQLRequestError::Invalid(
                "'query' parameter is missing".to_string(),
            ))
        })
}

async fn parse_post_json_req<S: ScalarValue>(
    body: Incoming,
) -> Result<GraphQLBatchRequest<S>, GraphQLRequestError> {
    let chunk = body
        .collect()
        .await
        .map_err(GraphQLRequestError::BodyHyper)?;

    let input = String::from_utf8(chunk.to_bytes().iter().cloned().collect())
        .map_err(GraphQLRequestError::BodyUtf8)?;

    serde_json::from_str::<GraphQLBatchRequest<S>>(&input)
        .map_err(GraphQLRequestError::BodyJSONError)
}

async fn parse_post_graphql_req<S: ScalarValue>(
    body: Incoming,
) -> Result<GraphQLBatchRequest<S>, GraphQLRequestError> {
    let chunk = body
        .collect()
        .await
        .map_err(GraphQLRequestError::BodyHyper)?;

    let query = String::from_utf8(chunk.to_bytes().iter().cloned().collect())
        .map_err(GraphQLRequestError::BodyUtf8)?;

    Ok(GraphQLBatchRequest::Single(GraphQLRequest::new(
        query, None, None,
    )))
}

pub fn graphiql(
    graphql_endpoint: &str,
    subscriptions_endpoint: Option<&str>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let mut resp = new_html_response(StatusCode::OK);
    // XXX: is the call to graphiql_source blocking?
    *resp.body_mut() =
        juniper::http::graphiql::graphiql_source(graphql_endpoint, subscriptions_endpoint).into();
    Ok(resp)
}

pub async fn playground(
    graphql_endpoint: &str,
    subscriptions_endpoint: Option<&str>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let mut resp = new_html_response(StatusCode::OK);
    *resp.body_mut() =
        juniper::http::playground::playground_source(graphql_endpoint, subscriptions_endpoint)
            .into();
    Ok(resp)
}

fn render_error(err: GraphQLRequestError) -> Response<Full<Bytes>> {
    let message = format!("{}", err);
    let mut resp = new_response(StatusCode::BAD_REQUEST);
    *resp.body_mut() = message.into();
    resp
}

async fn execute_request_sync<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<'static, QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    request: GraphQLBatchRequest<S>,
) -> Response<Full<Bytes>>
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
    let body = serde_json::to_string_pretty(&res).unwrap();
    let code = if res.is_ok() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    let mut resp = new_response(code);
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    *resp.body_mut() = body.into();
    resp
}

pub async fn execute_request<CtxT, QueryT, MutationT, SubscriptionT, S>(
    root_node: Arc<RootNode<'static, QueryT, MutationT, SubscriptionT, S>>,
    context: Arc<CtxT>,
    request: GraphQLBatchRequest<S>,
) -> Response<Full<Bytes>>
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
    let body = serde_json::to_string_pretty(&res).unwrap();
    let code = if res.is_ok() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    let mut resp = new_response(code);
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    *resp.body_mut() = body.into();
    resp
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

fn new_response(code: StatusCode) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::new()));
    *r.status_mut() = code;
    r
}

fn new_html_response(code: StatusCode) -> Response<Full<Bytes>> {
    let mut resp = new_response(code);
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
}

#[derive(Debug)]
enum GraphQLRequestError {
    BodyHyper(hyper::Error),
    BodyUtf8(FromUtf8Error),
    BodyJSONError(SerdeError),
    Variables(SerdeError),
    Invalid(String),
}

impl fmt::Display for GraphQLRequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            GraphQLRequestError::BodyHyper(ref err) => fmt::Display::fmt(err, f),
            GraphQLRequestError::BodyUtf8(ref err) => fmt::Display::fmt(err, f),
            GraphQLRequestError::BodyJSONError(ref err) => fmt::Display::fmt(err, f),
            GraphQLRequestError::Variables(ref err) => fmt::Display::fmt(err, f),
            GraphQLRequestError::Invalid(ref err) => fmt::Display::fmt(err, f),
        }
    }
}

impl Error for GraphQLRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            GraphQLRequestError::BodyHyper(ref err) => Some(err),
            GraphQLRequestError::BodyUtf8(ref err) => Some(err),
            GraphQLRequestError::BodyJSONError(ref err) => Some(err),
            GraphQLRequestError::Variables(ref err) => Some(err),
            GraphQLRequestError::Invalid(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use http_body_util::Full;
    use hyper::{Method, Response, StatusCode, server::conn::http1::Builder, service::service_fn};
    use hyper_util::rt::TokioIo;
    use juniper::{
        EmptyMutation, EmptySubscription, RootNode,
        http::tests as http_tests,
        tests::fixtures::starwars::schema::{Database, Query},
    };
    use reqwest::{self, blocking::Response as ReqwestResponse};
    use std::{net::SocketAddr, sync::Arc, thread, time::Duration};
    use tokio::net::TcpListener;
    use tokio::pin;
    use tokio::sync::broadcast;

    struct TestHyperIntegration {
        port: u16,
    }

    impl http_tests::HttpIntegration for TestHyperIntegration {
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

    async fn run_hyper_integration(is_sync: bool) {
        let port = if is_sync { 3002 } else { 3001 };
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();

        let db = Arc::new(Database::new());
        let root_node = Arc::new(RootNode::new(
            Query,
            EmptyMutation::<Database>::new(),
            EmptySubscription::<Database>::new(),
        ));

        let root_node = root_node.clone();
        let ctx = db.clone();

        let new_service = service_fn(move |req| {
            let root_node = root_node.clone();
            let ctx = ctx.clone();
            let matches = {
                let path = req.uri().path();
                match req.method() {
                    &Method::POST | &Method::GET => path == "/graphql" || path == "/graphql/",
                    _ => false,
                }
            };
            async move {
                if matches {
                    if is_sync {
                        super::graphql_sync(root_node, ctx, req).await
                    } else {
                        super::graphql(root_node, ctx, req).await
                    }
                } else {
                    let mut resp = Response::new(Full::new(Bytes::new()));
                    *resp.status_mut() = StatusCode::NOT_FOUND;
                    Ok(resp)
                }
            }
        });

        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
        let tx2 = shutdown_tx.clone();

        let (shutdown_fut, shutdown) = futures::future::abortable(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            shutdown_tx.send(());
        });

        let listener = TcpListener::bind(addr).await.unwrap();
        println!("Listening on http://{}", addr);
        tokio::task::spawn(async move {
            loop {
                let mut rx = shutdown_rx.resubscribe();
                let (tcp, remote_address) = tokio::select! {
                    res = listener.accept() => {
                        match res {
                            Ok((a,b)) => (a,b),
                            Err(e) =>{ println!("Error accepting connection: {:?}", e);
                            continue;
                            }
                        }
                    },
                    _ = rx.recv() => {
                        break;
                    }
                };

                let io = TokioIo::new(tcp);

                println!("accepted connection from {:?}", remote_address);

                let new_service = new_service.clone();

                let mut rx = shutdown_rx.resubscribe();
                tokio::task::spawn(async move {
                    let conn = Builder::new().serve_connection(io, new_service);
                    pin!(conn);
                    let shutdown_rx = rx.recv();
                    pin!(shutdown_rx);

                    tokio::select! {
                        res = conn.as_mut() => {
                            if let Err(e) = res {
                                return Err(e);
                            }
                            res
                        }
                        _ = shutdown_rx => {
                            println!("calling conn.graceful_shutdown");
                            conn.as_mut().graceful_shutdown();
                            Ok(())
                        }
                    };
                    Ok(())
                });
            }
        });

        tokio::task::spawn_blocking(move || {
            thread::sleep(Duration::from_millis(10)); // wait 10ms for server to bind
            let integration = TestHyperIntegration { port };
            http_tests::run_http_test_suite(&integration);
            shutdown.abort();
            tx2.send(());
        });
    }

    #[tokio::test]
    async fn test_hyper_integration() {
        run_hyper_integration(false).await
    }

    #[tokio::test]
    async fn test_sync_hyper_integration() {
        run_hyper_integration(true).await
    }
}
