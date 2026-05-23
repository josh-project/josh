# Step 7: Implement `handle_fetch()`

## Why

Fetch is the first phase of the merge queue cycle. It pulls the latest state from
all tracked remotes, discovers open PRs, and seeds admission state so that the
evaluation phase has accurate data.

## What to change

### File: `josh-cq/src/cq.rs`

Add a new public function:

```rust
pub fn handle_fetch(
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: CqActorState,
) -> anyhow::Result<CqActorState> {
```

#### Logic

1. **Enumerate tracked remotes** — Call `josh_core::link::find_link_files(repo, &head_tree)`
   to get all `.link.josh` files. For each, extract the `remote` and `commit` metadata
   fields. Build a `Vec<(String, String)>` of `(remote_url, current_commit_sha)`.

2. **Fetch from each remote** — For each remote URL, run:
   ```rust
   spawn_git_command(repo.path(), &["fetch", &url], &[])?;
   ```
   This brings the remote's objects (including PR head commits) into the metarepo's
   object store.

3. **Update `.link.josh` files** — For each remote, determine if the remote's HEAD
   has advanced. Use `josh_link::update_links()` to update `.link.josh` commit
   pointers. Follow the pattern in `josh_link::lib.rs`.

   Actually, the simplest approach: after fetch, get the new FETCH_HEAD, and if it
   differs from the current `.link.josh` commit, call `update_links()`.

4. **Discover open PRs** — For each remote URL:
   - Parse `owner`/`repo` from the URL using `josh_github_changes::repo::parse_owner_repo()`
   - Call `api.get_open_pull_requests(&owner, &repo)` (from Step 2)
   - For each open PR, insert it as a `CandidatePr` via `state.upsert_candidate()`

5. **Seed admission state** — For each open PR:
   - Call `state.get_or_init_pr_admission(&pr.node_id, &remote_url, api)` to create
     the `AdmissionState` entry and fetch required checks
   - Call `api.get_pr_reviews(&owner, &repo, pr.number)` (from Step 3) to get
     current review state
   - Feed the reviews into `admission.process_pr_review_events()` — but note that
     `process_pr_review_events` expects `&[PullRequestReviewEvent]`. You may need to
     synthesize minimal `PullRequestReviewEvent` structs from the review data, or
     add a new method `process_review_states()` on `AdmissionState` that accepts
     `&[(String, PullRequestReviewState)]` directly.

   For check runs, call `api.check_run_state_discover()` with the PR head SHAs
   (which requires GitHub node IDs, not SHAs — check the existing query; if it
   requires node IDs, you'll need to first get the commit node ID from the SHA).

6. **Return updated state** — `Ok(state)`

#### Simpler alternative for seeding

If synthesizing webhook events is awkward, add a method to `AdmissionState`
(in `forges/josh-github-changes/src/admission.rs`):

```rust
/// Directly set review state from fetched reviews (non-webhook path).
pub fn apply_review_states(
    &mut self,
    reviews: &[(String, josh_github_webhooks::webhook_types::PullRequestReviewState)],
) {
    for (login, state) in reviews {
        if self.maintainers.contains(login) {
            match state {
                PullRequestReviewState::Dismissed => {
                    self.maintainer_reviews.remove(login);
                }
                _ => {
                    self.maintainer_reviews.insert(login.clone(), state.clone());
                }
            }
        }
    }
}
```

Similarly for check runs:

```rust
/// Directly set check run results (non-webhook path).
pub fn apply_check_results(&mut self, results: &[(String, bool)]) {
    for (context, passed) in results {
        if let Some(entry) = self.required_checks.get_mut(&RequiredStatusCheck {
            context: context.clone(),
            integration_id: None,
        }) {
            *entry = *passed;
        }
    }
}
```

### Acceptance

- `cargo build --bin josh-cq` succeeds
- `cargo fmt` passes
- The function compiles but doesn't need to be called yet (wired in Step 10)
