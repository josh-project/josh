use josh_github_auth::device_flow::DeviceAuthFlow;
use josh_github_auth::middleware::GithubAuthMiddleware;

use reqwest_middleware::ClientBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let client_id = std::env::var("GITHUB_CLIENT_ID")
        .expect("set GITHUB_CLIENT_ID env var to run this example");

    let flow = DeviceAuthFlow::new(client_id.clone());

    // Step 1: request device code
    let device_code = flow.request_device_code().await?;
    let url = device_code
        .verification_uri_complete
        .as_deref()
        .unwrap_or(&device_code.verification_uri);

    println!("Open {} in your browser", url);
    println!("Code: {}", device_code.user_code);
    println!(
        "Waiting for authorization (expires in {}s)...",
        device_code.expires_in
    );

    // Step 2+3: poll until the user authorizes
    let token = flow.poll_for_token(&device_code, None).await?;
    println!("\nAuthorization successful!");
    println!("Token type: {}", token.token_type);
    println!("Scopes: {}", token.scope);

    // Build an HTTP client with the auth middleware
    let client = ClientBuilder::new(reqwest::Client::new())
        .with(GithubAuthMiddleware::from_app_flow(token, client_id))
        .build();

    // Make an authenticated request to the GitHub API
    let resp = client
        .get("https://api.github.com/user")
        .header(reqwest::header::USER_AGENT, "device-flow-example")
        .send()
        .await?;

    println!("\nGET /user → {}", resp.status());
    println!("{}", resp.text().await?);

    Ok(())
}
