# Plan: Full port of josh from git2 (libgit2) to gix (gitoxide)

## Context

Josh depends on git2/libgit2 (~1,231 references) for all repository I/O, while gix is already used
selectively where it wins: `josh-filter/src/persist.rs` (in-memory tree/blob construction +
`compute_hash`, batch-written to the git2 ODB), `read_parent_ids` in `josh-core/src/git.rs` (raw
ODB bytes + `gix_object::CommitRefIter`, avoiding libgit2's global parse-cache lock, measured ~3%
win), and commit rewriting in `history.rs`. A 2024 attempt (cd6dc206) at side-by-side gix+git2
repositories was reverted because two handles doing redundant I/O regressed high-transaction
workloads; the lesson is: gix for pure in-memory compute over a **single** ODB, full replacement
(not side-by-side) for the repository handle.

Goal: remove git2/libgit2-sys entirely, in a sequence of independently landable one-commit steps,
each verified to not regress the criterion/CodSpeed benches or change produced object bytes.
libgit2 pain points create headroom to bank along the way: eager whole-graph revwalk (documented
complaint at `history.rs:114-122`), the parse-cache lock, FFI overhead in the tree hot loop.

**gix API verification (done, against locked versions in the cargo registry — gix 0.79.0,
gix-object 0.56.0, gix-odb 0.76.0, gix-pack 0.66.0, gix-traverse 0.53.0):** no gix upgrade is a
prerequisite.

- Tree editing: `gix_object::tree::Editor::new(root, &dyn FindExt, hash_kind)` — takes any object
  source, no gix Repository needed. Does NOT validate entry names or normalize modes (see risk
  register).
- In-memory staging: `gix::OdbHandle` is `gix_odb::memory::Proxy<Handle>` with
  `with_object_memory()`; but its `reset_object_memory()` evicts immediately, which mismatches
  memodb's "readable until packed" semantics — so josh keeps its own memory layer (Phase 3).
- Object writes: `Repository::write_object/write_blob`; no dedup — keep persist.rs's `exists()`
  check. No per-object fsync (parity with libgit2 defaults).
- Pack writing: `gix_pack::data::output::Entry::from_data` + `Bundle::write_to_directory` +
  `index::File::write_data_iter_to_stream` — pack+idx from `(Kind, Vec<u8>)` buffers without any
  repo handle.
- Rev walk: high-level `rev_walk()` lacks topo+reverse, but
  `gix_traverse::commit::topo::Builder::new(find)` provides incremental (lazy) topo walk over any
  find impl, with `with_predicate` (≙ `with_hide_callback`), `parents(Parents::First)`
  (≙ `simplify_first_parent`), `with_commit_graph(Option<Graph>)`. Reverse = collect + `.rev()`
  (libgit2 buffers internally anyway).
- Refs: `references()?.prefixed(..)`, `edit_reference(s)` with `gix_ref::transaction::RefEdit`
  incl. packed-refs handling.
- Open: `gix::open(path)` does not search (≙ `open_ext NO_SEARCH`); `open::Options::isolated()`
  for config/env isolation. `ThreadSafeRepository` (Sync) + `.to_thread_local()` matches the
  Transaction model. `strict_object_creation(false)` needs no equivalent — gix never checks
  referenced-object existence on write.
- Commit-graph: read supported (`gix_commitgraph::Graph`); writing is not in gix — generate via
  the existing `spawn_git` path (`git commit-graph write`) in housekeeping.

## Architecture decisions

1. **Single-ODB hybrid.** The transition vehicle is a josh-owned adapter in josh-core
   (generalizing persist.rs): implements `gix_object::Find`/`FindExt`, backed pre-flip by
   `git2::Odb` raw reads + a `HashMap<ObjectId, (Kind, Vec<u8>)>` staging map, post-flip by
   `repo.objects` + the same map. All gix compute (tree editor, commit parsing, topo walk) plugs
   into this — never a second repo handle.
2. **Oid currency** converts module-by-module to `gix_hash::ObjectId` with zero-cost
   `as_bytes()`/`from_bytes()` at seams (the persist.rs pattern). The public
   `josh_core::Oid(git2::Oid)` wrapper (`josh-core/src/lib.rs:30`) keeps both `From` impls during
   transition; its inner type flips at cleanup.
3. **Seam narrowing before the flag day.** `Transaction::repo() -> &git2::Repository`
   (`cache/transaction.rs:215`) is the leak. Before flipping: (a) all object I/O behind the
   adapter, (b) all ref I/O behind new `Transaction` ref methods (implemented on git2 first). The
   flag-day commit then only touches `transaction.rs`, `git.rs`, wrapper internals.
4. **memodb** becomes pure Rust: the adapter's staging map is the authoritative memory store
   (memory-first read, disk fallback, byte-size tracking), the existing flusher-thread
   architecture stays but writes packs via gix-pack from a `Send` snapshot (no repo reopen on the
   worker). The libgit2 `#[repr(C)]` backend trampolines (`odb_backend.rs`) die with the crate.

## Step-by-step plan

Per-step verification baseline: `cargo bench -p josh-core` (8 benches; `EXPECTED_HEAD` asserts
prove byte-identical output) with criterion `--save-baseline` before/after, CodSpeed on master,
and `josh compose run` (178 prysk tests, which embed oids). Rollback everywhere: single-commit
revert; every step keeps both worlds compiling.

### Phase 0 — Safety nets
- **0.1 Bench-gap closure (lands first).** Add a write/flush-heavy bench (unapply/materialize
  over a large tree forcing memodb overflow flush — the Phase 3 steps are otherwise uncovered)
  and a ref-iteration bench for the josh-changes path. Record baselines.
  Files: `josh-core/benches/` + `Cargo.toml` bench entries.

### Phase 1 — Object compute onto gix-object; git2 remains sole I/O owner
- **1.1 `josh_core::objects` adapter.** `OdbView` implementing `gix_object::Find` over
  `git2::Odb` raw reads + staging map; `stage_blob/stage_tree/stage_commit` (compute_hash, no
  I/O); `flush_staged(&git2::Odb)` with exists-check. Refactor persist.rs onto it.
  Files: `josh-core/src/objects.rs` (new), `josh-core/src/lib.rs`, `josh-filter/src/persist.rs`.
  Verify: benches flat; prysk green.
- **1.2a–c Port `filter/tree.rs` (THE hot path), split by function family.** Replace
  `find_tree`/`treebuilder` with `gix_object::TreeRef` parsing + sorted-entry construction or
  `tree::Editor` over the adapter. a: `remove_pred`/insert paths; b: overlay/subtract/compose;
  c: pathstree/trigram/text helpers. Handle byte-identity explicitly: replicate libgit2's
  entry-name rejection (`.git`, `.`, `..`, empty, `/`) which `tree.rs:163` relies on via
  failed-insert behavior; normalize legacy modes via `EntryKind` (tree.rs flags
  `filemode_raw() != filemode()` at 165/433); git sort order via `gix_object::tree::Entry: Ord`.
  Verify: **all 8 benches flat-or-better, EXPECTED_HEAD unchanged** (byte-identity proof).
- **1.3 Port blob/commit creation in `filter/mod.rs`.** `repo.blob`/`git2::Signature`/
  `repo.commit` → `gix_object::Commit`/`gix_actor::Signature` staged via the adapter
  (`rewrite_commit` already proves CommitRef round-trips preserve gpgsig/extra headers). Risk:
  from-scratch commit serialization (signature/timezone formatting) — prysk oids enforce.
  Files: `josh-core/src/filter/mod.rs`, `josh-core/src/history.rs` (write path).
- **1.4 Kill per-commit `find_commit` in walk loops.** Extend the `read_parent_ids` pattern (raw
  bytes + `CommitRef`) to all per-commit loads. Banks the parse-cache-lock win.
  Files: `josh-core/src/history.rs`, `josh-core/src/git.rs`. Verify: deephistory_* flat-or-better.
- **1.5 Replace git2 revwalk with `gix_traverse::commit::topo::Builder`** backed by the adapter:
  `sorting(TopoOrder)`, `parents(Parents::First)`, `with_predicate` for hide callbacks,
  collect+`.rev()` for REVERSE. Banks the lazy-walk win (history.rs:114-122). Follow-up: spawn
  `git commit-graph write` in housekeeping + `with_commit_graph(Graph::at(..))`.
  Files: `josh-core/src/history.rs`. Verify: deephistory_* flat-or-better; prysk asserts oids
  (walk-order changes may shift cache hit patterns but not results).

### Phase 2 — Seam narrowing (refs + leaf crates), still git2-backed
- **2.1 Transaction ref API**: `update_ref/resolve_ref/for_each_ref_prefixed` on git2; migrate
  `update_refs` (`josh-core/src/lib.rs:169-221`).
- **2.2 josh-changes**: public fns `&git2::Repository` → `&Transaction`; internals onto ref API +
  `ObjectId` currency.
- **2.3–2.5 josh-graphql, josh-cli, josh-filter (`Filter::id()`, `LazyRef::Resolved`),
  josh-link/starlark/templates**: oid currency + Transaction routing. One PR per crate.
- **2.6 Audit**: remaining `transaction.repo()` callers must be only `transaction.rs`, `git.rs`,
  `housekeeping.rs`, memodb registration.
  Verify each: prysk; benches asserted flat.

### Phase 3 — memodb replacement and the repo-handle flag day
- **3.1 Flusher packs via gix-pack** (inside current memodb): `Entry::from_data` +
  `Bundle::write_to_directory` from a `Send` snapshot; no repo reopen on the worker.
  Files: `josh-memodb/src/{flusher,pack,mem_odb}.rs`. Verify: 0.1 flush bench flat-or-better;
  prysk (external `spawn_git` must see flushed objects).
- **3.2 Memory store moves into the adapter**: staging map becomes authoritative
  (`mem_odb_limit` byte tracking, overflow enqueue, boundary drain); unregister the libgit2 ODB
  backend (post-Phase-1, all hot-path reads/writes already flow through the adapter). git2 repo
  remains for refs + disk-fallback reads. Highest-risk perf step — compare baselines carefully.
  Files: `josh-core/src/objects.rs`, `josh-core/src/cache/transaction.rs:128-232`, josh-memodb.
- **3.3 Flag day: Transaction opens gix.** `TransactionContext` holds
  `ThreadSafeRepository::open_opts(path, Options::isolated())`; `Transaction` gets
  `.to_thread_local()` with tuned `object_cache_size`; adapter disk-fallback → `repo.objects`;
  ref API internals → `references().prefixed()`/`edit_references`; port `git.rs`
  env/signature/spawn paths. `Transaction::repo()` changes type — only in-crate callers remain.
  Files: `josh-core/src/cache/transaction.rs`, `git.rs`, `lib.rs`, `housekeeping.rs`.
  Verify: full suite; watch ref-heavy prysk tests (packed-refs) and Transaction::open latency.

### Phase 4 — Stragglers and removal
- **4.1** josh-proxy oid passthrough, josh-compose, josh-gui. One PR per crate.
- **4.2** Port bench setup code (`git2::build::TreeUpdateBuilder` → `repo.edit_tree()`);
  EXPECTED_HEAD values must not change — final byte-identity proof.
- **4.3** Flip `josh_core::Oid` inner type to `ObjectId`; delete `From<git2::Oid>` shims.
- **4.4** Delete josh-memodb (or reduce to the pure-Rust store); remove git2/libgit2-sys/
  vendored-libgit2 workspace-wide; `cargo tree -i git2` empty; full bench + prysk + CodSpeed.

## Byte-identity risk register

- Tree entry sort order, mode normalization, entry-name rejection — Step 1.2 (EXPECTED_HEAD +
  prysk).
- From-scratch commit serialization (signature/timezone) — Step 1.3 (prysk oids).
- Round-tripped commits (gpgsig, encoding, extra headers) — already proven by `rewrite_commit`.
- Pack-vs-loose representation never affects oids — Phase 3 needs functional verification only.

## Perf headroom banked

Lazy topo walk (1.5), no parse-cache lock (1.3/1.4), FFI-free in-memory tree staging (1.2),
commit-graph acceleration (post-1.5), flusher without repo reopen (3.1), build-time/binary-size
win from dropping vendored libgit2 (4.4).

## Critical files

- `josh-filter/src/persist.rs` — the adapter pattern to generalize
- `josh-core/src/filter/tree.rs` — hot path (Phase 1.2)
- `josh-core/src/history.rs` — walk + rewrite (1.3–1.5)
- `josh-core/src/cache/transaction.rs` — seam + flag day (Phase 3)
- `josh-memodb/src/mem_odb.rs` — semantics to preserve in the pure-Rust store
