use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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

#[derive(Serialize, Deserialize)]
struct SerializedChange {
    id: ChangeId,
    head: String,
}

#[derive(Serialize, Deserialize)]
struct SerializedEdge {
    from: ChangeId,
    to: ChangeId,
    base: String,
}

#[derive(Serialize, Deserialize)]
struct SerializedChangeGraph {
    changes: Vec<SerializedChange>,
    edges: Vec<SerializedEdge>,
}

impl Serialize for ChangeGraph {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let changes: Vec<SerializedChange> = self
            .graph
            .node_indices()
            .map(|idx| {
                let change = &self.graph[idx];
                SerializedChange {
                    id: change.id.clone(),
                    head: change.head.to_string(),
                }
            })
            .collect();

        let edges: Vec<SerializedEdge> = self
            .graph
            .edge_indices()
            .map(|idx| {
                let (from, to) = self.graph.edge_endpoints(idx).unwrap();
                let base = self.graph[idx];
                SerializedEdge {
                    from: self.graph[from].id.clone(),
                    to: self.graph[to].id.clone(),
                    base: base.to_string(),
                }
            })
            .collect();

        SerializedChangeGraph { changes, edges }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChangeGraph {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = SerializedChangeGraph::deserialize(deserializer)?;

        let mut graph = petgraph::graph::DiGraph::<Change, git2::Oid>::new();
        let mut nodes = HashMap::<ChangeId, NodeIndex>::new();

        for change in raw.changes {
            let oid = git2::Oid::from_str(&change.head).map_err(serde::de::Error::custom)?;
            let idx = graph.add_node(Change {
                id: change.id.clone(),
                head: oid,
            });
            nodes.insert(change.id, idx);
        }

        for edge in raw.edges {
            let from = *nodes.get(&edge.from).ok_or_else(|| {
                serde::de::Error::custom(format!("unknown change: {}", edge.from))
            })?;
            let to = *nodes
                .get(&edge.to)
                .ok_or_else(|| serde::de::Error::custom(format!("unknown change: {}", edge.to)))?;

            let base = git2::Oid::from_str(&edge.base).map_err(serde::de::Error::custom)?;
            graph.add_edge(from, to, base);
        }

        Ok(ChangeGraph { graph, nodes })
    }
}
