use std::collections::HashMap;

pub use petgraph::graph::NodeIndex;

pub type ChangeId = String;

pub struct Change {
    pub id: ChangeId,
    pub head: git2::Oid,
}

/// DAG (forest) of changes where edges point from child to parent.
/// Root-level changes (depending on trunk) have no outgoing edges.
///
/// The graph structure is based on change-id relationships, not git parent-child
/// relationships. This means the graph remains connected even if branches are
/// rebased or force-pushed, as long as change-ids are preserved.
///
/// Node type (`Change`): Represents a change with its current head commit oid.
///
/// Edge weight (`git2::Oid`): The oid of the git parent commit that the child
/// was originally based on. This doesn't define the graph structure but can be
/// used to detect whether a change needs rebasing (if the edge weight differs
/// from the parent change's current head, the child is based on an outdated
/// version of the parent).
pub struct ChangeGraph {
    pub graph: petgraph::graph::DiGraph<Change, git2::Oid>,
    pub nodes: HashMap<ChangeId, NodeIndex>,
}
