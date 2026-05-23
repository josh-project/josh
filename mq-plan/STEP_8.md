# Step 8: Implement candidate evaluation

## Why

After fetch populates the candidate pool and admission state, the queue needs to
select which PR to merge. The rule is simple: first admissible PR wins.

## What to change

### File: `josh-cq/src/cq.rs`

Add a function:

```rust
/// Select the first admissible PR from the candidate pool.
///
/// Iterates candidates in insertion order (BTreeMap), checks each one's
/// admission state, and returns the first that passes `admissible()`.
pub fn select_candidate(state: &CqActorState) -> Option<CandidatePr> {
    for (node_id, candidate) in &state.candidates {
        if let Some(admission) = state.pr_admissions.get(node_id) {
            if admission.admissible() {
                tracing::info!(
                    pr = %node_id,
                    number = candidate.number,
                    repo = %candidate.repo_url,
                    "selected admissible PR"
                );
                return Some(candidate.clone());
            }
        }
    }
    None
}
```

No changes to `AdmissionState` needed — `admissible()` already checks:
- At least one maintainer approved
- No maintainer requested changes
- All required checks passed

### Acceptance

- `cargo build --bin josh-cq` succeeds
- `cargo fmt` passes
