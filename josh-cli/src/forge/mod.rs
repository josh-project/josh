pub mod github;

pub use josh_changes::remote_config::Forge;

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
