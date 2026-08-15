//! Terminal output formatting helpers.

/// Build the web URL for a PR number on the target repo.
pub(crate) fn pr_web_url(url: &str, number: i64) -> Option<String> {
    let (owner, repo_name) = crate::repo::parse_owner_repo(url).ok()?;
    Some(format!(
        "https://github.com/{}/{}/pull/{}",
        owner, repo_name, number
    ))
}

/// Wrap `text` in an OSC 8 terminal hyperlink (iTerm2, kitty, WezTerm, ...)
/// when stderr is a TTY; return `text` unchanged otherwise.
pub(crate) fn hyperlink(url: &str, text: &str) -> String {
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, text)
    } else {
        text.to_string()
    }
}

/// Render a `PR #N` reference, hyperlinked to the PR when on a TTY.
pub(crate) fn pr_link(url: &str, number: i64) -> String {
    let text = format!("PR #{}", number);
    match pr_web_url(url, number) {
        Some(web_url) => hyperlink(&web_url, &text),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_web_url_from_git_urls() {
        assert_eq!(
            pr_web_url("https://github.com/octocat/hello-world", 42).unwrap(),
            "https://github.com/octocat/hello-world/pull/42"
        );
        assert_eq!(
            pr_web_url("git@github.com:octocat/hello-world.git", 42).unwrap(),
            "https://github.com/octocat/hello-world/pull/42"
        );
        assert!(pr_web_url("https://gitlab.com/octocat/hello-world", 42).is_none());
    }

    #[test]
    fn hyperlink_is_plain_text_when_not_a_tty() {
        // Tests never run with stderr on a TTY, so the text comes back plain.
        assert_eq!(hyperlink("https://github.com/o/r/pull/1", "PR #1"), "PR #1");
    }
}
