//! Forge identification for links and remotes.

/// Forge-specific behavior for a link or remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Forge {
    Github,
    Gerrit,
}

impl std::fmt::Display for Forge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Forge::Github => f.write_str("github"),
            Forge::Gerrit => f.write_str("gerrit"),
        }
    }
}
