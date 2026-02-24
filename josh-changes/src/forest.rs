use std::collections::BTreeMap;

pub type ChangeId = String;

struct ChangeNode {
    patch: (git2::Oid, git2::Oid),
    metadata: git2::Oid,
}

pub struct Change {
    pub id: ChangeId,
    pub patch: (git2::Oid, git2::Oid),
    pub metadata: git2::Oid,
}

// Arena graph of changes; ephemeral -- exists only in the
// process of changes being materialized to some sort of concrete
// backend, e.g. set of refs + PRs
pub struct ChangeForest {
    changes: BTreeMap<ChangeId, ChangeNode>,
    heads: Vec<ChangeId>,
    edges: BTreeMap<ChangeId, Vec<ChangeId>>,
}

impl ChangeForest {
    pub fn new() -> Self {
        ChangeForest {
            changes: BTreeMap::new(),
            heads: Vec::new(),
            edges: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, change: Change, parents: impl IntoIterator<Item = ChangeId>) {
        let parents: Vec<ChangeId> = parents.into_iter().collect();

        for parent in &parents {
            self.heads.retain(|h| h != parent);
        }

        self.changes.insert(
            change.id.clone(),
            ChangeNode {
                patch: change.patch,
                metadata: change.metadata,
            },
        );

        self.edges.insert(change.id.clone(), parents);
        self.heads.push(change.id);
    }

    /// Remove and return a change that has no unresolved dependencies.
    /// Returns `None` when the forest is empty.
    pub fn pop_next(&mut self) -> Option<Change> {
        let ready = self
            .edges
            .iter()
            .find(|(_, parents)| parents.is_empty())
            .map(|(id, _)| id.clone())?;
        self.edges.remove(&ready);

        let node = self.changes.remove(&ready)?;
        self.heads.retain(|h| h != &ready);

        for deps in self.edges.values_mut() {
            deps.retain(|d| d != &ready);
        }

        Some(Change {
            id: ready,
            patch: node.patch,
            metadata: node.metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_change(id: &str) -> Change {
        Change {
            id: id.into(),
            patch: (git2::Oid::zero(), git2::Oid::zero()),
            metadata: git2::Oid::zero(),
        }
    }

    #[test]
    fn pop_respects_dependencies() -> anyhow::Result<()> {
        //   a
        //  / \
        // b   c
        //  \ /
        //   d
        let mut f = ChangeForest::new();

        f.insert(dummy_change("a"), std::iter::empty());
        f.insert(dummy_change("b"), vec!["a".into()]);
        f.insert(dummy_change("c"), vec!["a".into()]);
        f.insert(dummy_change("d"), vec!["b".into(), "c".into()]);

        let mut order = Vec::new();
        while let Some(c) = f.pop_next() {
            order.push(c.id);
        }

        assert_eq!(order.len(), 4);
        // a must come before b, c, and d
        // b and c must come before d
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));

        Ok(())
    }
}
