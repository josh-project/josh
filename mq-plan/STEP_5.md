# Step 5: Add `CandidatePr` model and extend `CqActorState`

## Why

The merge queue needs to track which open PRs exist, their metadata (head SHA,
base branch, repo URL), and cross-reference them with `AdmissionState` to decide
which PR to merge. Currently `CqActorState` only has `admission` (required checks
per repo) and `pr_admissions` (admission state per PR node ID). There's no place
to store PR metadata.

## What to change

### File: `josh-cq/src/cq.rs`

1. Add a `CandidatePr` struct near the top of the file (next to `CqActorState`):

```rust
#[derive(Debug, Clone)]
pub struct CandidatePr {
    pub node_id: String,
    pub number: i64,
    pub repo_url: String,
    pub head_sha: String,
    pub head_branch: String,
    pub base_sha: String,
    pub base_branch: String,
    pub title: String,
}
```

2. Add a `candidates` field to `CqActorState`:

```rust
#[derive(Default, Clone)]
pub struct CqActorState {
    pub admission: BTreeMap<String, BTreeSet<RequiredStatusCheck>>,
    pub pr_admissions: BTreeMap<String, AdmissionState>,
    pub candidates: BTreeMap<String, CandidatePr>,  // keyed by PR node_id
}
```

3. Add helper methods on `CqActorState`:

```rust
impl CqActorState {
    /// Insert or update a candidate PR.
    pub fn upsert_candidate(&mut self, pr: CandidatePr) {
        self.candidates.insert(pr.node_id.clone(), pr);
    }

    /// Remove a candidate PR and its admission state.
    pub fn remove_candidate(&mut self, pr_node_id: &str) {
        self.candidates.remove(pr_node_id);
        self.pr_admissions.remove(pr_node_id);
    }

    /// Get a candidate by PR node ID.
    pub fn get_candidate(&self, pr_node_id: &str) -> Option<&CandidatePr> {
        self.candidates.get(pr_node_id)
    }
}
```

### Acceptance

- `cargo build --bin josh-cq` succeeds
- No warnings
- `cargo fmt` passes
