use anyhow::{anyhow, Context};
use graphql_client::GraphQLQuery;
use http::header;

use crate::connection::GithubApiConnection;

pub const GITHUB_ACCEPT: &str = "application/vnd.github+json";
pub const GITHUB_REST_API_URL: &str = "https://api.github.com";
pub const GITHUB_GRAPHQL_API_URL: &str = "https://api.github.com/graphql";
pub const GITHUB_API_VERSION: &str = "2022-11-28";

pub const JOSH_API_USER_AGENT: &str = "josh-project";

impl GithubApiConnection {
    // Non-generic code to actually make the request -
    // reduces the amount of compiled templated code
    async fn do_send_request(&self, body: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let builder = self
            .client
            .post(self.api_url.clone())
            .header(header::USER_AGENT, JOSH_API_USER_AGENT)
            .header(header::ACCEPT, GITHUB_ACCEPT);

        let response = builder
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .header("X-Github-Next-Global-ID", "1")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if status.is_success() {
            let response_body = response.json().await?;
            Ok(response_body)
        } else {
            let response_body = response.text().await?;
            let message = format!(
                "Unexpected status code while making request: {} {}",
                self.api_url, status
            );

            if response_body.trim().is_empty() {
                Err(anyhow!(message))
            } else {
                Err(anyhow!("{}, body: {}", message, response_body))
            }
        }
    }

    pub(crate) async fn make_request<T: GraphQLQuery>(
        &self,
        variables: T::Variables,
    ) -> anyhow::Result<T::ResponseData> {
        let request_body = T::build_query(variables);
        let request_body = serde_json::to_value(&request_body)?;

        let response = self.do_send_request(request_body).await?;
        let response_body: graphql_client::Response<T::ResponseData> =
            serde_json::from_value(response).context("Failed to parse response body")?;

        if let Some(errors) = response_body.errors {
            let message = errors
                .iter()
                .map(|error| error.message.clone())
                .collect::<Vec<_>>()
                .join(", ");

            return Err(anyhow!("GraphQL request failed: {:?}", message));
        }

        response_body.data.context("Response data is missing")
    }
}
