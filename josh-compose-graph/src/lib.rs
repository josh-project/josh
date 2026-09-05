//! Build-graph data structures and loading for `josh-compose`.
//!
//! [`load_graph`] resolves the full workspace and image dependency closure from
//! git trees into an in-memory [`Graph`], before any execution starts. Executors
//! (see the `Executor` trait in `josh-compose-backend`) consume the loaded graph
//! and never walk git trees themselves; only heavy byte payloads (build-context
//! and worktree tars) stay lazy, addressed by tree OID.

mod graph;
mod meta;
mod visualize;

pub use graph::{Graph, ImageNode, Job, load_graph};
pub use meta::{SidecarSpec, WorkspaceMeta};

/// Whether a job produces an output artifact and whether it is extracted.
#[derive(Debug, Clone, PartialEq)]
pub enum OutputMode {
    /// No output artifact is created; only success/failure is recorded.
    None,
    /// Output artifact is created and its contents are extracted to the host working directory.
    Workdir,
    /// Output artifact is created and kept (e.g. for use as a dependency input), but not
    /// extracted.
    Keep,
}

/// Network reachability a step requests (independent of sidecars).
///
/// When a step has sidecar workers, the backend connects the step to them
/// regardless of this policy.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkPolicy {
    /// No network access.
    None,
    /// Full host network access.
    Host,
}
