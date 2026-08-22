# git2 -> gix port: status

Last updated: 2026-08-16. See `PLAN.md` in this directory for the full phased plan.

## Landed on master (one commit per step, each suite-green and bench-validated)

| Step | Commit | What | Perf vs pre-gix baseline |
|------|--------|------|--------------------------|
| 0.1 | b5f6428d | New gate benches: `deephistory_prefix_flush` (object write/flush path), `refs_filter_update` (ref enumeration/update) | n/a (baseline recorded here) |
| 1.1 | 77cd799e | New `josh-gix-ext` crate: `StagingOdb` adapter (gix `Find`/`FindHeader`/`Exists` over git2, zero-cost oid/kind conversion, batch flush with exists-check); persist.rs refactored onto it; re-exported as `josh_core::objects`; lives in its own crate below every other josh crate | flat |
| 1.2a | fc3f8d6b | `remove_pred`/`remove_pattern` ported to gix tree parsing + raw odb writes | glob benches **-5 to -17%** |
| 1.2b | a075fed2 | `subtract`/`intersect`/`overlay` ported; per-transaction tree-bytes cache (`Transaction::read_tree_bytes`, policy in `cache::tree_cache::TreeCache`: Arc<[u8]>, PassthroughHasher-keyed, promote-on-second-access, zero-copy first read, 64 MB budget) | `ultrawide_pin_hook` **-1 to -33%** |
| 1.2c | 1bbda9e4 | `pathstree`/`regex_replace`/`insert` ported (return `git2::Tree` via one `find_tree` at the tail until `Rewrite` goes oid-based) | flat-or-better (`prefix_flush/1000` -2.6%) |
| 1.4 | 5db00bd8 | `apply_to_commit2` + history walk loops to oid currency: owned `CommitData` (id + raw bytes, parse-on-demand `CommitRef`) loaded once after the memo gate (memo hits = zero I/O); `Rewrite::from_commit_data` leaves author/committer/message unparsed (None slots -> `rewrite_commit` keeps the raw bytes verbatim; canonical signatures re-serialized identically, so typical output is unchanged, and non-canonical ones now pass through instead of being normalized -- deliberate divergence); no signature parsing anywhere: `:author`/`:committer` re-serialize a NameEmail pair with the base's timestamp, Squash-ids transplants the squash-result's raw signature fields verbatim (`SigRewrite::Raw`), `:message` decodes the message at point of use; `rewrite_commit(&CommitData)` drops its duplicate odb read; `read_tree_id` sibling of `read_parent_ids`; filtered-parent slot cache now serves Prune/Fold/pin | `deephistory_subdir` **-25 to -28%**, `deephistory_glob` **-22 to -49%**, `widetree_glob` **-14 to -63%**, `ultrawide_pin_hook` **-17 to -56%**, `prefix_flush` **-15 to -18%**, `refs_filter_update` **-2 to -4%**, rest flat |
| 1.5 | a54d5b1d | Native walkers in josh-gix-ext, replacing `git2::Revwalk` (gix_traverse topo::Builder refuted: eager without commit-graph, `with_predicate` cannot prune traversal; commits parsed only to their parent headers, committer lines never read). `objects::RevWalk`: pruned topo walk (DFS reverse-postorder, per-commit prune = context-free ancestor-closed cache knowledge, first-parent, lazy `discover` pre-order stream replacing `find_unapply_base`'s RefCell/error-smuggling hide-callback hack; order deterministic and pinned by unit tests -- first-match callers pick by it). `objects::RangeWalk`: exact `base..tip` (a contextual reachability boundary no cache lookup can answer); requires a sequence-number lookup at construction; fast-forward chain path (O(delta), lookup untouched, so linear pushes never grow the hint cache) or ranked highest-seq-first frontier stopping when only hidden entries pend (bounds merge/rebase shapes at the merge-base level); `unapply_filter` passes a computing `compute_sequence_number` lookup (the hint walk is a plain pruned RevWalk, so no recursion); `downstack` needs no RangeWalk -- on a first-parent spine prune-at-base equals excluding the base's history, so it uses a first-parent RevWalk and errors on an off-spine base. Yield sets pinned by an in-test reference model + `git2::Revwalk` set equality for RevWalk, over fixed shapes and seeded random DAGs, incl. a boundedness proof for the ranked walk. Scrut: 8 expectation updates (hint counters/pack listings -- roots blobs now written for filtered history on non-linear pushes). | step-level flat-to-better; cumulative vs pre-gix: `widetree_glob_prefix/50000` **-62%**, `ultrawide_pin_hook` **-16 to -54%**, `deephistory_glob_recursive` **-43 to -47%**, `deephistory_subdir` **-21 to -23%**, `prefix_flush` **-13 to -16%**, `refs_filter_update` **-2 to -3%** |
| 2.1 | 32340023 | Transaction ref API: `resolve_ref` (Option-not-error, symrefs followed, unpeeled), `update_ref` (always force, explicit log message), `for_each_ref_prefixed` (prefix-not-glob, byte-sorted -- the one deliberate observable change -- skips symbolic/non-UTF-8/corrupt refs); flag-day contract pinned in the method doc comments AND by unit tests (incl. a hand-packed ref exercising the loose+packed sort). Ported: `update_refs`, `list_refs`/`default_from_to`/`memorize_from_to` (now `&Transaction`), `discover_filter_candidates` (prefix + `ends_with(".git/HEAD")`; the dead `josh/filtered/` loop kept verbatim), `get_info`, josh-proxy caller, refs_filter_update bench. NOT covered (deliberate): `git.rs` (allowlisted), `cache/distributed.rs` (own repo below the cache stack -- needs its own port step in phase 3, plan gap) | `refs_filter_update` **-3 to -4%**, rest at post-1.5 levels, no regressions |

| 2.2 | c3320d47 | josh-changes onto the Transaction ref API: all public fns take `&Transaction` (redundant `(repo, transaction)` pairs collapsed; `Change::new`/`create_synthetic_merge_commit` take oids, `Change::new` now fallible); ref reads via `resolve_ref`, enumeration via two `for_each_ref_prefixed` calls, and every `repo.commit(Some(&ref))` split into `repo.commit(None, ..)` + `Transaction::update_ref`, reshaped to take an `Expected` guard (`Any`/`Absent`/`At(oid)`, mirroring gix `PreviousValue`); `At`/`Absent` preserve libgit2's first-parent-must-be-tip check on the metadata refs (contract pinned by 6 unit tests incl. packed-ref match and error-on-vanished). Object I/O, git2 revwalks, `repo.head()` (symbolic HEAD) and `revparse_single` stay on `transaction.repo()` until flag day (`// PORT:` markers). Deliberate divergences: corrupt-ref errors propagate instead of reading as absent (2.1 contract), symbolic refs under `refs/josh/{changes,remotes}/` no longer enumerate, reflog message text differs. Callers ported: josh-cli, josh-proxy, josh-github-changes (+josh-core dep), josh-gui (+josh-core dep; per-callback `TransactionContext` over an empty `CacheStack`) | flat (`refs_filter_update` no-change vs pre-gix; josh-core hot paths untouched) |

Phase 1.2 is complete: no `treebuilder` use remains in `josh-core/src/filter/tree.rs`.

Phase 1.3 (commit/blob creation) required no work: every commit write already goes through
`rewrite_commit` (`history.rs:262`), which is already gix-based (CommitRef round-trip + raw odb
write); blob writes are plain odb writes that memodb intercepts.

## Tree rewriting semantics (in `josh-gix-ext` / `filter/tree.rs`)

- `tree_entry_name_valid`: git's own `core.protectNTFS`/`core.protectHFS` entry-name rules,
  applied only to names josh itself introduces (insert/replace_child silently no-op, overlay's
  strict tree2 side errors). Input entries are never name-validated -- `.git` aliases read from
  input trees pass through byte-for-byte; checkout protection is the git client's job.
- Rebuilds are byte-preserving (treebuilder-byte-preservation): kept entries carry their raw
  modes, mode spellings and original relative order, duplicates included, so a fully-kept tree
  round-trips verbatim even when fsck-invalid; `write_tree_now` serializes entries in exactly
  the order given, and freshly inserted entries are placed at their canonical position.
  `TreeRebuild` owns the unchanged fast path (`changed` flag) for entry-by-entry rebuilds.
- `lookup_entry`: two-probe `bisect_entry` (blob probe first, matching canonical order), the
  `Tree::get_name` equivalent; non-canonical entry order is detected once per parsed tree and
  falls back to a linear scan, where bisection would mis-resolve.
- All pinned by unit tests in `filter/tree.rs` and `objects.rs`.

## Next steps

1. **2.2** josh-changes: public fns `&git2::Repository` -> `&Transaction`; internals onto the
   ref API + `ObjectId` currency. Then **2.3-2.5** josh-graphql, josh-cli, josh-filter
   (`Filter::id()`, `LazyRef::Resolved`), josh-link/starlark/templates; **2.6** audit
   (remaining `transaction.repo()` callers only transaction.rs, git.rs, housekeeping.rs
   object reads, memodb registration). Ref-usage maps for all leaf crates are in
   `.agents/work/gix-port/2.1-research.md`. The ref API will need deletion/symbolic-create
   additions when josh-proxy migrates (`upstream.rs` fake-head delete, `service.rs`
   `reference_symbolic`).
2. **Phase 3** Flusher packs via gix-pack; memory store into the adapter; flag day: Transaction
   opens `gix::ThreadSafeRepository` (isolated), refs via gix-ref (parity contract pinned in
   the 2.1 method comments + unit tests). Plan gap found in 2.1: `cache/distributed.rs` holds
   its own `git2::Repository` (one ref write, two resolves) below the cache stack -- needs its
   own port sub-step here.
3. **Phase 4** Port bench setup, flip `josh_core::Oid` inner type, delete josh-memodb FFI,
   remove git2/libgit2-sys workspace-wide (`cargo tree -i git2` empty).

Deferred from 1.5 (evaluate separately): lazy early-exit walks for
find_original/find_oldest_similar/find_new_branch_base (change tie-breaks -> test churn);
commit-graph write in housekeeping (no consumer yet). Pre-existing bugs found during
research, file/fix separately: Op::Unlink missing-queue spin (filter/mod.rs ~790 returns
Ok(None) without recording a missing entry -> driver-loop spin on malformed .link.josh);
discover_filter_candidates' filtered-refs loop has matched nothing since introduction
(prefix misses the leading "refs/", kept verbatim in 2.1).

## Validation protocol (per step)

- Criterion `pre-gix` baselines live in `target/criterion` (recorded at the 0.1 benches
  commit, pristine master). Compare with a loop of
  `cargo bench -p josh-core --bench <b> -- --baseline pre-gix` over the 11 bench targets
  (incl. `unapply`, whose two groups pin the push-path walkers: `unapply_extend` must stay
  O(push length), `unapply_new_branch` scales with the discover walk).
  Do NOT use `--benches` (the lib target rejects criterion flags).
- Per-commit deltas across the local stack: `.agents/work/gix-port/stack-bench-run.sh`
  saves a baseline per step; `.agents/work/gix-port/stack-bench-report.py` prints the
  consecutive-delta table. Uniform few-percent shifts across unrelated benches between
  steps are machine drift -- trust deltas aligned with what a commit touches.
- Never edit or build anything while a bench run is in flight (cargo rebuilds mid-loop and
  poisons the measurement). Benches run right after a `josh compose run` read ~3% high --
  let the machine settle (~2 min) or rerun flagged cases before believing a regression.
- Before `josh compose run`: `cargo fmt`, `cargo test -p josh-core --lib`, and
  `cargo check --workspace` (Transaction must stay `Send` -- no `Rc` in it; josh-graphql
  requires it).
- `josh compose run` needs `JOSH_EXPERIMENTAL_FEATURES=1`; don't pipe it through `tail`
  (truncates the log). Exit 0 + `[test suite] Done (orchestrator)` means everything passed,
  including the per-category prysk jobs.
