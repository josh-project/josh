use std::fmt::{Display, Formatter};

use clap::ValueEnum;

pub const GITHUB_APP_CLIENT_ID: &str = "Iv23liK2qIIUHy5iILiz";
pub const GITHUB_APP_ID: &str = "2871336";

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
    let url = url::Url::parse(url).ok()?;
    let host = url.host_str()?;

    if host == "github.com" || host.ends_with(".github.com") {
        return Some(Forge::Github);
    }

    None
}

/// Parse owner and repository name from a GitHub URL.
/// Supports https://github.com/owner/repo[.git] and git@github.com:owner/repo[.git].
pub fn parse_github_owner_repo(url: &str) -> anyhow::Result<(String, String)> {
    let url = url.trim();
    if let Some(stripped) = url.strip_prefix("git@github.com:") {
        let path = stripped.strip_suffix(".git").unwrap_or(stripped);
        let mut parts = path.splitn(2, '/');
        let owner = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid GitHub SSH URL: {}", url))?;
        let repo = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid GitHub SSH URL: {}", url))?;
        return Ok((owner.to_string(), repo.to_string()));
    }
    let parsed = url::Url::parse(url).map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;
    if parsed
        .host_str()
        .map_or(true, |h| h != "github.com" && !h.ends_with(".github.com"))
    {
        return Err(anyhow::anyhow!("Not a GitHub URL: {}", url));
    }
    let path = parsed.path().trim_start_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut segments = path.splitn(2, '/');
    let owner = segments
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL: {}", url))?;
    let repo = segments
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL: {}", url))?;
    Ok((owner.to_string(), repo.to_string()))
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
