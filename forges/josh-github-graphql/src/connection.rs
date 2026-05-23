use url::Url;

use josh_github_auth::middleware::GithubAuthMiddleware;

pub struct GithubApiConnection {
    pub client: reqwest_middleware::ClientWithMiddleware,
    pub api_url: Url,
}

impl GithubApiConnection {
    /// Construct with an explicit client and API URL (for testing).
    pub fn new(client: reqwest_middleware::ClientWithMiddleware, api_url: Url) -> Self {
        Self { client, api_url }
    }

    /// Construct without authentication, pointed at a custom URL (for testing).
    pub fn for_test(api_url: Url) -> Self {
        Self::new(
            reqwest_middleware::ClientBuilder::new(reqwest::Client::new()).build(),
            api_url,
        )
    }

    pub fn from_token(token: String) -> Self {
        let middleware = GithubAuthMiddleware::from_token(token);
        let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with(middleware)
            .build();
        Self {
            client,
            api_url: crate::request::GITHUB_GRAPHQL_API_URL
                .parse()
                .expect("GITHUB_GRAPHQL_API_URL is valid"),
        }
    }

    pub fn from_environment() -> Option<Self> {
        if let Ok(token) = std::env::var("GH_TOKEN") {
            if !token.is_empty() {
                tracing::info!("using GH_TOKEN for GitHub API connection");
                return Some(Self::from_token(token));
            }
        }

        let stored = josh_github_keyring::load_stored_token()?;
        tracing::info!("using stored device-flow token for GitHub API connection");

        let middleware = GithubAuthMiddleware::from_app_flow(
            stored,
            josh_github_auth::APP_CLIENT_ID.to_string(),
        );

        let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with(middleware)
            .build();

        Some(Self {
            client,
            api_url: crate::request::GITHUB_GRAPHQL_API_URL
                .parse()
                .expect("GITHUB_GRAPHQL_API_URL is valid"),
        })
    }
}
