use std::fmt::{Display, Formatter};

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Forge {
    /// GitHub
    Github,
}

impl Display for Forge {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Forge::Github => f.write_str("github"),
        }
    }
}

pub fn guess_forge(url: &str) -> Option<Forge> {
    if josh_github_auth::is_github_url(url) {
        return Some(Forge::Github);
    }

    None
}

pub mod github {
    use josh_github_auth::APP_CLIENT_ID;
    use josh_github_auth::middleware::GithubAuthMiddleware;
    use josh_github_auth::token::load_stored_token;
    use josh_github_graphql::connection::GithubApiConnection;

    // Matches official github CLI and other github-adjacent tools
    pub const GITHUB_USER_TOKEN_ENV: &str = "GH_TOKEN";

    pub async fn make_api_connection() -> Option<GithubApiConnection> {
        let middleware = if let Ok(token) = std::env::var(GITHUB_USER_TOKEN_ENV) {
            GithubAuthMiddleware::from_token(token)
        } else {
            let stored = load_stored_token()?;
            GithubAuthMiddleware::from_app_flow(stored, APP_CLIENT_ID.to_string())
        };

        let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with(middleware)
            .build();

        Some(GithubApiConnection {
            client,
            api_url: url::Url::parse("https://api.github.com").unwrap(),
        })
    }

    pub fn api_connection_hint() -> String {
        format!(
            "Couldn't create API connection; log in to GitHub with 'josh auth login github', or set {} environment variable",
            GITHUB_USER_TOKEN_ENV
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::forge::{Forge, guess_forge};

    #[test]
    fn test_guess_forge() {
        assert_eq!(
            guess_forge("https://github.com/josh-project/josh.git"),
            Some(Forge::Github)
        )
    }
}
