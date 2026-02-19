use std::fmt::{Display, Formatter};

use clap::ValueEnum;

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
