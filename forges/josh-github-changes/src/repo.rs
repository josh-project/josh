/// Parse owner and repository name from a GitHub URL.
/// Supports https://github.com/owner/repo[.git] and git@github.com:owner/repo[.git].
pub fn parse_owner_repo(url: &str) -> anyhow::Result<(String, String)> {
    let url = url.trim();

    let path = if let Some(stripped) = url.strip_prefix("git@github.com:") {
        stripped.to_string()
    } else {
        let parsed = url::Url::parse(url).map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;
        if parsed
            .host_str()
            .map_or(true, |h| h != "github.com" && !h.ends_with(".github.com"))
        {
            return Err(anyhow::anyhow!("Not a GitHub URL: {}", url));
        }

        parsed.path().trim_start_matches('/').to_string()
    };

    let path = path.strip_suffix(".git").unwrap_or(&path);
    let (owner, repo) = path
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL (missing owner/repo): {}", url))?;

    Ok((owner.to_string(), repo.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_urls() {
        let cases = [
            "https://github.com/octocat/hello-world",
            "https://github.com/octocat/hello-world.git",
            "git@github.com:octocat/hello-world",
            "git@github.com:octocat/hello-world.git",
            "  https://github.com/octocat/hello-world  ",
        ];
        for url in cases {
            let (owner, repo) = parse_owner_repo(url).unwrap_or_else(|e| panic!("{url}: {e}"));

            assert_eq!(owner, "octocat", "{url}");
            assert_eq!(repo, "hello-world", "{url}");
        }
    }

    #[test]
    fn invalid_urls() {
        let cases = [
            "https://gitlab.com/octocat/hello-world",
            "https://github.com/octocat",
            "not a url at all",
        ];

        for url in cases {
            assert!(parse_owner_repo(url).is_err(), "{url} should be rejected");
        }
    }
}
