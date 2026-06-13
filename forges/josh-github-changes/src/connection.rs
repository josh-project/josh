use josh_github_auth::middleware::GithubAuthMiddleware;
use josh_github_auth::APP_CLIENT_ID;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::request::GITHUB_GRAPHQL_API_URL;

// Matches official github CLI and other github-adjacent tools
pub const GITHUB_USER_TOKEN_ENV: &str = "GH_TOKEN";

pub async fn make_api_connection() -> Option<GithubApiConnection> {
    let middleware = if let Ok(token) = std::env::var(GITHUB_USER_TOKEN_ENV) {
        GithubAuthMiddleware::from_token(token)
    } else {
        let stored = josh_github_keyring::load_stored_token()?;
        GithubAuthMiddleware::from_app_flow(stored, APP_CLIENT_ID.to_string())
    };

    let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
        .with(middleware)
        .build();

    Some(GithubApiConnection {
        client,
        api_url: GITHUB_GRAPHQL_API_URL
            .parse()
            .expect("Failed to parse API URL"),
    })
}

pub fn api_connection_hint() -> String {
    format!(
        "Couldn't create API connection; log in to GitHub with 'josh auth login github', or set {} environment variable",
        GITHUB_USER_TOKEN_ENV
    )
}
