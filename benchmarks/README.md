To run the benchmark:

```
pixi run select-folders
```

## history-rewrites

Native comparison of Josh, git-filter-repo, git filter-branch, and Copybara.
Every tool rewrites the Josh repository with the same three operations:

- `subtree`: extract `josh-core/` to the repository root
- `prefix`: move the full tree below `project/`
- `glob`: retain non-hidden `*.rs` files at any depth

Josh and git-filter-repo also run against git/git, using `builtin/` and
`*.c` files. The intentionally slow git filter-branch and Copybara runs are
excluded from that larger repository.

```
pixi run history-rewrites
```

Each sample uses fresh tool state. Before timing, every selected executable is
exercised against a tiny throwaway repository and the measured repository's
commit and tree objects are traversed. This avoids giving whichever tool runs
first a cold executable or filesystem page-cache penalty; “cold” refers to
tool state. By default, the
cold phase rewrites history through `HEAD~100`; the incremental phase then
advances the same output to `HEAD`, using the tool's existing state. Set
`HISTORY_REWRITE_INCREMENT_COMMITS` to change that first-parent distance.
git filter-branch has no persistent incremental cache, so its catch-up DAG is
staged on top of the cold filtered result before timing; only the new range is
then passed to filter-branch. Repository preparation and building
`josh-filter` from the latest published Josh release happen outside the timed
sections. The exact Josh release, source commits, and actual range commit count
are recorded with the results.

The default is one sample because the full git filter-branch run is expensive.
Use `HISTORY_REWRITE_RUNS` for repeated samples. Comma-separated selectors can
limit repositories, tools, or transformations:

```
HISTORY_REWRITE_RUNS=3 \
HISTORY_REWRITE_INCREMENT_COMMITS=100 \
HISTORY_REWRITE_REPOS=josh \
HISTORY_REWRITE_TOOLS=josh,git-filter-repo \
HISTORY_REWRITE_TRANSFORMS=subtree,prefix,glob \
pixi run history-rewrites
```

Set `HISTORY_REWRITE_JOSH_REV` or `HISTORY_REWRITE_GIT_REV` to a remote ref or
full commit SHA to reproduce a source revision. For quick command and output
validation, `HISTORY_REWRITE_SMOKE=1` replaces the two range endpoints with a
two-commit snapshot history while retaining their real repository trees:

```
HISTORY_REWRITE_SMOKE=1 HISTORY_REWRITE_REPOS=josh \
pixi run history-rewrites
```

Results land in `target/history_rewrites_samples.json`,
`target/history_rewrites_samples.csv`, and `target/history_rewrites.png`.

## rustc-josh-sync

End-to-end replay of real rust-lang subtree syncs (rustc-dev-guide, stdarch,
compiler-builtins, miri), both directions, cold and warm cache. Two tools are
compared: `josh-proxy` (the production setup, server-side filtering) and the
`josh` CLI (client-side filtering). Select with `SYNC_TOOLS=proxy,cli`.

```
pixi run rustc-josh-sync
```

Quick subset run:

```
SYNC_REPLAY_DEPTH=3 SYNC_SUBTREES=miri pixi run rustc-josh-sync
```

To benchmark a different josh version (e.g. for regression comparisons), pass
its commit SHA; all caches are keyed by it:

```
SYNC_JOSH_COMMIT=<sha> pixi run rustc-josh-sync
```

Results land in `target/rustc_josh_sync_samples.json` and
`target/rustc_josh_sync.png`. The first run fetches rust-lang/rust (cached
under `target/rust-lang-source/`) and mines the historical sync events from it
(cached as `target/rustc-josh-sync/manifest-*.json`).
