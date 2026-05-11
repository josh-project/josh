use url::Url;

pub struct GithubApiConnection {
    pub client: reqwest_middleware::ClientWithMiddleware,
    pub api_url: Url,
}

impl GithubApiConnection {
    pub fn from_token(token: String) -> Self {
        let middleware = josh_github_auth::middleware::GithubAuthMiddleware::from_token(token);
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
}
