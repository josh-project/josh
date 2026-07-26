# git2 -> gix port: status

Last updated: 2026-07-26. See `PLAN.md` in this directory for the full phased plan.

## Landed on master (one commit per step, each suite-green and bench-validated)

| Step | Commit | What | Perf vs pre-gix baseline |
|------|--------|------|--------------------------|
| 0.1 | 684fc500 | New gate benches: `deephistory_prefix_flush` (object write/flush path), `refs_filter_update` (ref enumeration/update) | n/a (baseline recorded here) |
| 1.1 | f796ac56 | New `josh-gix-ext` crate: `StagingOdb` adapter (gix `Find`/`FindHeader`/`Exists` over git2, zero-cost oid/kind conversion, batch flush with exists-check); persist.rs refactored onto it; re-exported as `josh_core::objects`; lives in its own crate below every other josh crate | flat |
| 1.2a | 7c0646a6 | `remove_pred`/`remove_pattern` ported to gix tree parsing + raw odb writes | glob benches **-5 to -17%** |
| 1.2b | e5dd2215 | `subtract`/`intersect`/`overlay` ported; per-transaction tree-bytes cache (`Transaction::read_tree_bytes`: Arc<[u8]>, FxHash, promote-on-second-access, zero-copy first read, 64 MB budget) | `ultrawide_pin_hook` **-1 to -33%** |
| 1.2c | e388a940 | `pathstree`/`regex_replace`/`insert` ported (return `git2::Tree` via one `find_tree` at the tail until `Rewrite` goes oid-based) | flat-or-better (`prefix_flush/1000` -2.6%) |

Phase 1.2 is complete: no `treebuilder` use remains in `josh-core/src/filter/tree.rs`.

Phase 1.3 (commit/blob creation) required no work: every commit write already goes through
`rewrite_commit` (`history.rs:262`), which is already gix-based (CommitRef round-trip + raw odb
write); blob writes are plain odb writes that memodb intercepts.

## Byte-identity parity helpers (in `josh-gix-ext` / `filter/tree.rs`)

- `tree_entry_name_valid`: libgit2 `valid_entry_name` incl. NTFS `.git` aliases (always) and HFS
  aliases (now on all platforms; libgit2 checked HFS only on Apple -- deliberately stricter).
- `normalize_filemode`: libgit2-exact, incl. the quirk that any exec bit wins over the
  symlink/gitlink type checks.
- `push_deduped` / `seed_entries`: the name-keyed treebuilder's last-wins duplicate handling,
  scan gated on detected entry-order violations so canonical trees never pay for it.
- `lookup_entry`: two-probe `bisect_entry` (blob probe first, matching canonical order), the
  `git_tree_entry_byname` equivalent.
- Seeded rebuilds (subtract/overlay/replace_child) keep raw legacy modes of untouched entries
  byte-for-byte; replaced/inserted entries get normalized modes; overlay errors on invalid names
  while subtract/intersect drop them silently -- all matching the old treebuilder behavior and
  pinned by unit tests in `filter/tree.rs` and `objects.rs`.

## Next steps

1. **1.4** Change `apply_to_commit2(filter, &git2::Commit, transaction)` (`filter/mod.rs:587`)
   to take an oid + raw-parsed `gix_object::CommitRef`, killing per-commit `find_commit` in
   `walk2`/`find_unapply_base`/`get_rev_filter` (`history.rs:27,58,102,166,240,378,...`).
   Ripples through `Rewrite::from_commit` and the splice logic -- a full work unit.
2. **1.5** Replace the git2 revwalk with `gix_traverse::commit::topo::Builder` backed by the
   adapter (`with_predicate` replaces hide callbacks; `find_unapply_base`'s
   incremental-processing-during-graph-load hack becomes a plain early-exit loop; collect+rev
   for REVERSE). Follow-up: `git commit-graph write` in housekeeping + `with_commit_graph`.
3. **Phase 2** Transaction ref API (`update_ref`/`resolve_ref`/`for_each_ref_prefixed`), then
   josh-changes/graphql/cli/link off git2 types. Note: josh-link and josh-changes call
   `tree::insert(repo, ...)` -- kept repository-based until they get transactions in 2.2.
4. **Phase 3** Flusher packs via gix-pack; memory store into the adapter; flag day: Transaction
   opens `gix::ThreadSafeRepository` (isolated), refs via gix-ref.
5. **Phase 4** Port bench setup, flip `josh_core::Oid` inner type, delete josh-memodb FFI,
   remove git2/libgit2-sys workspace-wide (`cargo tree -i git2` empty).

## Validation protocol (per step)

- Criterion `pre-gix` baselines live in `target/criterion` (recorded at 684fc500, pristine
  master). Compare with a loop of
  `cargo bench -p josh-core --bench <b> -- --baseline pre-gix` over the 10 bench targets.
  Do NOT use `--benches` (the lib target rejects criterion flags).
- Never edit or build anything while a bench run is in flight (cargo rebuilds mid-loop and
  poisons the measurement). Benches run right after a `josh compose run` read ~3% high --
  let the machine settle (~2 min) or rerun flagged cases before believing a regression.
- Before `josh compose run`: `cargo fmt`, `cargo test -p josh-core --lib`, and
  `cargo check --workspace` (Transaction must stay `Send` -- no `Rc` in it; josh-graphql
  requires it).
- `josh compose run` needs `JOSH_EXPERIMENTAL_FEATURES=1`; don't pipe it through `tail`
  (truncates the log). Exit 0 + `[test suite] Done (orchestrator)` means everything passed,
  including the per-category prysk jobs.
