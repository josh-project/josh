use clap::ValueEnum;
use std::fmt::Formatter;

pub mod github;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Forge {
    /// GitHub
    Github,
    /// Gerrit
    Gerrit,
}

impl std::fmt::Display for Forge {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Forge::Github => f.write_str("github"),
            Forge::Gerrit => f.write_str("gerrit"),
        }
    }
}

/// How `josh changes publish` maps a stack onto Gerrit changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum GerritMode {
    /// Push only dependency-free changes, each as its own independent review.
    /// Changes that still have unmerged dependencies wait until those merge.
    #[default]
    Independent,
    /// Push the whole commit history once as a single Gerrit relation chain.
    Stack,
}

impl std::fmt::Display for GerritMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GerritMode::Independent => f.write_str("independent"),
            GerritMode::Stack => f.write_str("stack"),
        }
    }
}

pub fn guess_forge(url: &str) -> Option<Forge> {
    if josh_github_auth::is_github_url(url) {
        return Some(Forge::Github);
    }

    None
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
