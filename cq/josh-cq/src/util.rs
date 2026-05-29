use anyhow::Context;

/// Build a git signature for commits created by the merge queue.
///
/// Honours `JOSH_COMMIT_TIME` (used by tests for deterministic OIDs); otherwise
/// falls back to the repository's configured signature.
pub fn make_signature(repo: &git2::Repository) -> anyhow::Result<git2::Signature<'static>> {
    if let Ok(time) = std::env::var("JOSH_COMMIT_TIME") {
        git2::Signature::new(
            "JOSH",
            "josh@josh-project.dev",
            &git2::Time::new(time.parse().context("Failed to parse JOSH_COMMIT_TIME")?, 0),
        )
        .context("Failed to create signature")
    } else {
        let sig = repo.signature().context("Failed to get signature")?;
        Ok(sig.to_owned())
    }
}
