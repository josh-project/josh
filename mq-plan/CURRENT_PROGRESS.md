# Merge Queue Implementation Progress

| Step | Status | Commit | Description |
|------|--------|--------|-------------|
| [STEP_1.md](STEP_1.md) | completed | `d2ead61c` | Remove `fetch`/`step`/`push` CLI subcommands |
| [STEP_2.md](STEP_2.md) | completed | — | Add GraphQL query to list open PRs |
| [STEP_3.md](STEP_3.md) | completed | `b7dbf833` | Add GraphQL query to fetch PR reviews |
| [STEP_4.md](STEP_4.md) | completed | `934f5b44` | Add GraphQL mutation to post PR comment |
| [STEP_5.md](STEP_5.md) | completed | `4b146723` | Add `CandidatePr` model and extend `CqActorState` |
| [STEP_6.md](STEP_6.md) | completed | `a885c7f3` | Handle `PullRequest` and `Push` webhook events |
| [STEP_7.md](STEP_7.md) | completed | `8b53e876` | Implement `handle_fetch()` |
| [STEP_8.md](STEP_8.md) | completed | — | Implement candidate evaluation |
| [STEP_9.md](STEP_9.md) | completed | — | Implement `handle_step()` |
| [STEP_10.md](STEP_10.md) | completed | `47e936e2` | Wire actor loop with tick timer into `serve` |
| [STEP_11.md](STEP_11.md) | completed | `9ef6b7b6` | Split `cq.rs` into modules |
| [STEP_12.md](STEP_12.md) | completed | `cce0eed0` | Create `josh-test-github` crate with `TestRepo` and `GitServer` |
| [STEP_13.md](STEP_13.md) | completed | `fd7681ad` | Add `SimRepo` and webhook event generation |
| [STEP_14.md](STEP_14.md) | pending | — | Add mock GraphQL server |
| [STEP_15.md](STEP_15.md) | pending | — | Add `GithubApiConnection` constructors, wire API override |
| [STEP_16.md](STEP_16.md) | pending | — | Write integration tests |
