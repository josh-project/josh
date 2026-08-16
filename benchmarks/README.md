To run the benchmark:

```
pixi run select-folders
```

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
