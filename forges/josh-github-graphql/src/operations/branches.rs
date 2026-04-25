use anyhow::anyhow;
use http::header;

use crate::connection::GithubApiConnection;
use crate::request::{GITHUB_ACCEPT, GITHUB_API_VERSION, GITHUB_REST_API_URL, JOSH_API_USER_AGENT};

impl GithubApiConnection {
    /// Lists all remote refs whose path starts with `prefix` (e.g. `"heads/@changes"`).
    /// Returns branch names with the `refs/heads/` prefix stripped.
    /// Handles GitHub's pagination automatically.
    pub async fn list_refs_by_prefix(
        &self,
        owner: &str,
        repo: &str,
        prefix: &str,
    ) -> anyhow::Result<Vec<String>> {
        let base = format!(
            "{}/repos/{}/{}/git/refs/{}",
            GITHUB_REST_API_URL, owner, repo, prefix
        );
        let mut results = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!("{}?per_page=100&page={}", base, page);
            let response = self
                .client
                .get(&url)
                .header(header::USER_AGENT, JOSH_API_USER_AGENT)
                .header(header::ACCEPT, GITHUB_ACCEPT)
                .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
                .send()
                .await?;

            let status = response.status();
            if status == http::StatusCode::NOT_FOUND {
                // No refs matching the prefix — that's fine
                break;
            }
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("list_refs_by_prefix failed ({}): {}", status, body));
            }

            // Check for a next page before consuming the body
            let has_next_page = response
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .map_or(false, |link| link.contains(r#"rel="next""#));

            let refs: serde_json::Value = response.json().await?;
            let arr = match refs.as_array() {
                Some(a) => a,
                None => break,
            };

            for r in arr {
                if let Some(ref_name) = r.get("ref").and_then(|v| v.as_str()) {
                    let branch = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
                    results.push(branch.to_string());
                }
            }

            if !has_next_page {
                break;
            }
            page += 1;
        }

        Ok(results)
    }

    /// Deletes a single remote branch by name (without `refs/heads/` prefix).
    /// Uses `DELETE /repos/{owner}/{repo}/git/refs/heads/{branch}`.
    pub async fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/repos/{}/{}/git/refs/heads/{}",
            GITHUB_REST_API_URL, owner, repo, branch
        );
        let response = self
            .client
            .delete(&url)
            .header(header::USER_AGENT, JOSH_API_USER_AGENT)
            .header(header::ACCEPT, GITHUB_ACCEPT)
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "delete_branch '{}' failed ({}): {}",
                branch,
                status,
                body
            ));
        }

        Ok(())
    }
}
