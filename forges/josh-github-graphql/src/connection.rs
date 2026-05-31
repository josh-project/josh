use std::sync::Arc;

use url::Url;

use josh_github_auth::middleware::GithubAuthMiddleware;

pub struct GithubApiConnection {
    pub client: reqwest_middleware::ClientWithMiddleware,
    pub api_url: Url,
}

impl GithubApiConnection {
    /// Build an authenticated connection from a shared auth middleware, pointed
    /// at the real GitHub GraphQL API. The same `Arc` can be reused elsewhere
    /// (e.g. to attach tokens to spawned git commands).
    pub fn from_middleware(middleware: Arc<GithubAuthMiddleware>, api_url: Option<Url>) -> Self {
        let api_url = api_url.unwrap_or(crate::request::GITHUB_GRAPHQL_API_URL
            .parse()
            .expect("GITHUB_GRAPHQL_API_URL is valid"));

        let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with_arc(middleware)
            .build();

        Self { client, api_url }
    }
}
