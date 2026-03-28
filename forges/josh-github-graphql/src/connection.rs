use url::Url;

pub struct GithubApiConnection {
    pub client: reqwest_middleware::ClientWithMiddleware,
    pub api_url: Url,
}
