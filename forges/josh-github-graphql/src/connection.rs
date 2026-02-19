use std::sync::Arc;

use reqwest::header;
use reqwest_middleware::{ClientBuilder, Middleware, Next};
use url::Url;

use crate::request::GITHUB_GRAPHQL_API_URL;

pub struct GithubApiConnection {
    pub client: reqwest_middleware::ClientWithMiddleware,
    pub api_url: Url,
}

/// Middleware that adds a Bearer token to requests.
struct BearerAuthMiddleware {
    token: Arc<str>,
}

#[async_trait::async_trait]
impl Middleware for BearerAuthMiddleware {
    async fn handle(
        &self,
        mut req: reqwest::Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        req.headers_mut().insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", self.token.as_ref()))
                .map_err(|e| reqwest_middleware::Error::Middleware(e.into()))?,
        );
        next.run(req, extensions).await
    }
}

impl GithubApiConnection {
    /// Create a connection with the given access token for GitHub API authentication.
    pub fn with_token(access_token: String) -> anyhow::Result<Self> {
        let api_url = Url::parse(GITHUB_GRAPHQL_API_URL)?;
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(BearerAuthMiddleware {
                token: Arc::from(access_token.into_boxed_str()),
            })
            .build();
        Ok(Self { client, api_url })
    }
}
