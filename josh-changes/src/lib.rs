//! Stacked-changes metadata store for josh: change identity, comments, votes,
//! revisions, and stacked-changes push machinery, persisted in `refs/josh/...`
//! refs.
//!
//! The public API is flat: everything is re-exported at the crate root.

pub use josh_core::trailers::{commit_change_meta, parse_change_meta};

mod change;
mod comments;
pub mod layout;
mod refs;
mod revisions;
mod stacked;
mod store;
mod votes;

pub use change::*;
pub use comments::*;
pub use refs::*;
pub use revisions::*;
pub use stacked::*;
pub use store::*;
pub use votes::*;
