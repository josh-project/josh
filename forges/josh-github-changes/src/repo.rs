/// Parse owner and repository name from a GitHub URL.
/// Supports https://github.com/owner/repo[.git] and git@github.com:owner/repo[.git].
pub fn parse_owner_repo(url: &str) -> anyhow::Result<(String, String)> {
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
