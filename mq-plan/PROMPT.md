# Merge Queue Implementation

You are implementing the Josh merge queue step by step. Work through the steps
in `mq-plan/` sequentially, one commit per step.

## Before starting

Read these files to understand the codebase and plan:

1. `CLAUDE.md` — project overview, conventions (always `cargo fmt` before commit,
   keep PRs to one commit, etc.)
2. `mq-plan/OVERVIEW.md` — architecture, design decisions, crate map
3. `mq-plan/CURRENT_PROGRESS.md` — track which steps are done and which is next

## How to proceed

1. Read `mq-plan/CURRENT_PROGRESS.md`. Find the first step marked `pending`.
2. Read `mq-plan/STEP_N.md` for that step.
3. Mark the step as `in_progress` in `mq-plan/CURRENT_PROGRESS.md`.
4. Implement the changes described in the step file.
5. Run `cargo build` to verify compilation.
6. Run `cargo fmt`.
7. Create a single git commit with a message summarizing the step.
8. Mark the step as `completed` in `mq-plan/CURRENT_PROGRESS.md` and commit the
   progress update.
9. Stop. (The next agent invocation will pick up the next step.)

## Rules

- **One step per invocation.** Do not chain multiple steps in one run.
- **One commit per step.** Follow the commit style: short imperative subject line,
  optional body with context. See `git log` for examples.
- **Read before editing.** Always read a file before using Edit on it.
- **Stay focused.** Only implement what the current step describes. Don't drift
  into refactoring or polishing unrelated code.
- **When in doubt about a GraphQL schema field name**, check the schema file at
  `forges/josh-github-codegen-graphql/src/github.graphql`.
- **After a GraphQL step** (steps 2-4), the generated code lives in the codegen
  crate's `OUT_DIR`. You may need `cargo build` to trigger codegen before the
  graphql crate can reference the new types.
- **Follow existing patterns.** Copy the style of adjacent code — same error
  handling, same logging approach (`tracing::info!` / `tracing::error!`), same
  import grouping.
- **Commit message format.** Use the project convention. The commit subject
  should describe the change, not reference "Step N". Example:
  ```
  Add GraphQL query to list open pull requests
  ```

  Not:
  ```
  Step 2: add get_open_prs query
  ```

- **For the test crate steps (12-16)**:
  - The test crate lives at `forges/josh-test-github/`
  - The reference implementation is at `../../metahead/metahead/mh-github-testrepo/`
  - Webhook types come from `josh-github-webhooks` (NOT from `mh-github`)
  - GraphQL client types come from `josh-github-graphql` and the codegen crate
  - GraphQL mock must respond to `graphql_client::QueryBody` requests with
    `graphql_client::Response<T::ResponseData>` JSON shapes
  - Use `reqwest::blocking::Client` for webhook sending (same pattern as metahead)
  - Use `axum` for HTTP servers (GitServer, hook listener, GraphQL mock)
