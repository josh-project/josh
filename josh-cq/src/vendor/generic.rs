use std::collections::HashMap;

use crate::change::{Change, ChangeGraph, ChangeId};
use crate::vendor::ChangeVendor;

/// Single commit changes only, requires change-id for a commit
/// to be considered a change
pub struct GenericRemote {
    pub repo_path: std::path::PathBuf,
    pub url: String,
    pub trunk_ref: String,
}

impl GenericRemote {}

fn get_change_id(commit: &git2::Commit) -> Option<ChangeId> {
    let find_change_id = |text: &str| -> Option<String> {
        text.lines().find_map(|line| {
            let line = line.to_lowercase();
            let line = line.trim();

            line.strip_prefix("change:")
                .or_else(|| line.strip_prefix("change-id:"))
                .and_then(|change_id| {
                    let change_id = change_id.trim();

                    if change_id.is_empty() {
                        None
                    } else {
                        Some(change_id.to_string())
                    }
                })
        })
    };

    if let Some(message) = commit.message()
        && let Some(change_id) = find_change_id(message)
    {
        return Some(change_id);
    }

    if let Some(headers) = commit.raw_header()
        && let Some(change_id) = find_change_id(headers)
    {
        return Some(change_id);
    }

    None
}

impl ChangeVendor for GenericRemote {
    fn list_changes(&self) -> anyhow::Result<ChangeGraph> {
        let repo = git2::Repository::open(&self.repo_path)?;
        let fetched = crate::remote::fetch(&repo, &self.url)?;

        // Consider every branch as a stack of changes as long as all ancestors
        // leading to trunk have change-id

        // Get all branches except trunk
        let heads = fetched
            .iter()
            .filter(|(ref_name, _)| {
                ref_name.starts_with("refs/heads") && **ref_name != self.trunk_ref
            })
            .collect::<Vec<_>>();

        let trunk = repo.find_reference(&self.trunk_ref)?.peel_to_commit()?;

        // Find branches that eventually connect to trunk
        let mut connected_heads = Vec::new();
        for (_, head_oid) in heads {
            let merge_base = match repo.merge_base(*head_oid, trunk.id()) {
                Ok(base) => base,
                Err(e) if e.code() == git2::ErrorCode::NotFound => {
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            connected_heads.push((*head_oid, merge_base))
        }

        // Filter out for branches where all commits have known change-id
        let mut change_stacks = Vec::<Vec<(git2::Oid, ChangeId)>>::new();
        'branch: for (head_oid, merge_base) in connected_heads {
            let mut revwalk = repo.revwalk()?;
            revwalk.push(head_oid)?;
            revwalk.hide(merge_base)?;

            let mut stack = Vec::new();
            for oid in revwalk {
                let oid = oid?;
                let commit = repo.find_commit(oid)?;

                let Some(change_id) = get_change_id(&commit) else {
                    // Skip this branch if any commit lacks a change-id
                    continue 'branch;
                };

                stack.push((oid, change_id));
            }

            if !stack.is_empty() {
                change_stacks.push(stack);
            }
        }

        // Build the change graph
        let mut graph = petgraph::graph::DiGraph::<Change, git2::Oid>::new();
        let mut nodes = HashMap::<ChangeId, petgraph::graph::NodeIndex>::new();

        // First pass: create all nodes
        for stack in &change_stacks {
            for (oid, change_id) in stack {
                if let Some(&node_idx) = nodes.get(change_id) {
                    // TODO: what to do if same change-id was encountered twice?
                    graph[node_idx].head = *oid;
                } else {
                    let node_idx = graph.add_node(Change {
                        id: change_id.clone(),
                        head: *oid,
                    });
                    nodes.insert(change_id.clone(), node_idx);
                }
            }
        }

        // Second pass: create edges based on git parent's change-id
        for stack in &change_stacks {
            for (oid, change_id) in stack {
                let commit = repo.find_commit(*oid)?;

                // Get the first git parent
                let Ok(parent) = commit.parent(0) else {
                    continue;
                };

                let parent_oid = parent.id();

                // Check if parent has a change-id that's in our graph
                let Some(parent_change_id) = get_change_id(&parent) else {
                    continue;
                };

                let Some(&parent_node) = nodes.get(&parent_change_id) else {
                    continue;
                };

                let &child_node = nodes.get(change_id).unwrap();

                // Edge from child to parent, weighted by the git parent oid
                graph.add_edge(child_node, parent_node, parent_oid);
            }
        }

        Ok(ChangeGraph { graph, nodes })
    }
}
