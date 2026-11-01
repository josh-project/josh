//! How `josh changes publish` maps a change stack onto a remote's refs.

use crate::remote_config::Forge;

/// How `josh changes publish` maps a change stack onto a remote's refs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishMode {
    /// The stack is split into per-change refs
    /// (`@changes`/`@base`/`@heads/<target>/<author>/...`): the publishing
    /// style of forges that review each change from its own branch
    /// (GitHub), and the default for refs-only remotes.
    BranchBased,
    /// No ref splitting: the branch is pushed as-is (the test forge, where
    /// the whole stack lives on one branch) or mapped onto the forge's own
    /// magic refs (Gerrit's `refs/for/*`).
    PushBased,
}

impl PublishMode {
    /// The publish mode of a remote's configured forge.
    pub fn for_forge(forge: Option<Forge>) -> PublishMode {
        match forge {
            Some(Forge::Gerrit) | Some(Forge::Test) => PublishMode::PushBased,
            Some(Forge::Github) | None => PublishMode::BranchBased,
        }
    }
}
