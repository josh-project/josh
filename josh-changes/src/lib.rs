//! Stacked-changes metadata store for josh: change identity, comments, votes,
//! revisions, and forge-specific publishing (GitHub-style stacked refs and
//! Gerrit), persisted in `refs/josh/...` refs.
//!
//! The public API is flat: everything is re-exported at the crate root.

pub use josh_core::trailers::{commit_change_meta, parse_change_meta};

mod change;
mod comments;
mod forges;
mod refs;
mod revisions;
mod store;
mod union;
mod votes;

pub use change::*;
pub use comments::*;
pub use forges::*;
pub use refs::*;
pub use revisions::*;
pub use store::*;
pub use union::*;
pub use votes::*;
