use josh_github_auth::middleware::GithubAuthMiddleware;

use reqwest_middleware::ClientBuilder;
use secret_vault_value::SecretValue;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let app_id =
        std::env::var("GITHUB_APP_ID").expect("set GITHUB_APP_ID env var to run this example");
    let installation_id = std::env::var("GITHUB_INSTALLATION_ID")
        .expect("set GITHUB_INSTALLATION_ID env var to run this example");
    let key_path = std::env::var("GITHUB_PRIVATE_KEY_PATH")
        .expect("set GITHUB_PRIVATE_KEY_PATH env var to run this example");

    let key_pem = std::fs::read(&key_path)
        .unwrap_or_else(|e| panic!("failed to read private key from {}: {}", key_path, e));
    let key = SecretValue::from(key_pem);

    eprintln!("Authenticating as GitHub App {}...", app_id);

    let middleware = GithubAuthMiddleware::from_github_app(app_id, installation_id, key).await?;

    let client = ClientBuilder::new(reqwest::Client::new())
        .with(middleware)
        .build();

    // Make an authenticated request as the installation
    let resp = client
        .get("https://api.github.com/installation/repositories")
        .header(reqwest::header::USER_AGENT, "app-flow-example")
        .send()
        .await?;

    eprintln!("GET /installation/repositories → {}", resp.status());
    eprintln!("{}", resp.text().await?);

    Ok(())
}
