# Step 9: Implement `handle_step()`

## Why

This is the core of the merge queue: take an admissible PR, merge it locally in
the metarepo, push the result to the remote's main branch, update `.link.josh`,
and close the PR on GitHub.

## What to change

### File: `josh-cq/src/cq.rs`

Add a new public function:

```rust
pub fn handle_step(
    candidate: &CandidatePr,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
```

#### Logic

1. **Identify the remote's current main commit** — From the `.link.josh` file for
   `candidate.repo_url`, read the `commit` metadata field. This is the current
   HEAD of the remote's main branch from the metarepo's perspective. Call it
   `main_sha`.

2. **Get the merge base**:
   ```rust
   let merge_base = spawn_git_command(
       repo.path(),
       &["merge-base", &main_sha, &candidate.head_sha],
       &[],
   )?;
   let merge_base = merge_base.trim().to_string();
   ```

3. **Compute the merged tree** using `git merge-tree`:
   ```rust
   let merged_tree = spawn_git_command(
       repo.path(),
       &["merge-tree", &merge_base, &main_sha, &candidate.head_sha],
       &[],
   )?;
   // merge-tree prints the tree OID to stdout
   let merged_tree = merged_tree.trim().to_string();
   ```

   Note: `git merge-tree` writes the tree OID to stdout. If there are conflicts,
   it still succeeds but includes conflict markers in the output. Check if the
   output looks like a valid OID (40 hex chars). If not, the merge has conflicts
   and we should skip this PR.

4. **Create the merge commit** using `git commit-tree`:
   ```rust
   let message = format!("Merge PR #{}: {}", candidate.number, candidate.title);
   let merge_commit = spawn_git_command(
       repo.path(),
       &[
           "commit-tree",
           "-p", &main_sha,
           "-p", &candidate.head_sha,
           "-m", &message,
           &merged_tree,
       ],
       &[],
   )?;
   let merge_commit = merge_commit.trim().to_string();
   ```

5. **Push to the remote's main branch**:
   ```rust
   let refspec = format!("{}:refs/heads/main", merge_commit);
   spawn_git_command(
       repo.path(),
       &["push", &candidate.repo_url, &refspec],
       &[],
   )?;
   ```

   Determine the target branch name from the `.link.josh` metadata or the
   remote's default branch. The remote URL might need credentials; `git push`
   will use the credential helper configured by `josh auth login`.

6. **Update `.link.josh` in the metarepo** — Call `josh_link::update_links()`
   with the new merge commit OID for the path corresponding to this remote.
   This creates a new tree with the updated link file.

7. **Create a metarepo commit** — Commit the updated `.link.josh` file to the
   metarepo. Follow the pattern from `handle_track()`:
   ```rust
   let signature = make_signature(repo)?;
   let head_commit = repo.head()?.peel_to_commit()?;
   // Use update_links result to create the commit, or do it manually
   ```

   Actually, `josh_link::update_links()` already creates a commit. Check its
   return type: `UpdateLinksResult` has `commit_with_updates` and `filtered_commit`.
   We should advance the metarepo HEAD to `commit_with_updates`.

   Update the metarepo's HEAD:
   ```rust
   repo.head()?
       .set_target(update_result.commit_with_updates, "josh-cq merge")?;
   ```

8. **Post comment and close PR on GitHub** (if API is available):
   ```rust
   if let Some(api) = api {
       let (owner, repo_name) =
           josh_github_changes::repo::parse_owner_repo(&candidate.repo_url)?;
       let comment = format!(
           "Merged by Josh merge queue as `{}`.",
           merge_commit
       );
       // Step 4's method:
       api.add_pr_comment(&candidate.node_id, &comment).await?;
       // Already exists:
       api.close_pull_request(&candidate.node_id).await?;
   }
   ```

   `add_pr_comment` and `close_pull_request` need an async context. Since
   `handle_step` is called from a `spawn_blocking` context, use
   `tokio::runtime::Handle::current().block_on(...)` (the same pattern used
   in `get_or_fetch_admission` and `lookup_open_prs_by_sha`).

9. **Remove the candidate** from the state:
   ```rust
   state.remove_candidate(&candidate.node_id);
   ```

#### Conflict handling

Before step 4, check if `merged_tree` looks like a tree OID (40 hex characters).
If `git merge-tree` outputs conflict markers, log a warning and return `Ok(())`
without merging (the PR stays in the candidate pool; it will be retried on the
next cycle after the author rebases).

### Acceptance

- `cargo build --bin josh-cq` succeeds
- `cargo fmt` passes
- The function compiles (not called yet; wired in Step 10)
