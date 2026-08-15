Reverse-filter roundtrip for a trivial merge that straddles a `:rev` cutoff.

The original repo has a root-layout region (`a` at the repo root) up to a cutoff commit, then a
nested region (`sub/a`) after it. The filter reconstructs a continuous root-layout history:


For commits `<=CUTOFF` (root layout) `:prefix=sub` then `:/sub` is the identity; for commits past
the cutoff (nested) `:rev` is a no-op and `:/sub` extracts the subdirectory. On the filtered side we
author a trivial merge whose first parent maps back to a `<=CUTOFF` (root-layout) commit while the
merge itself is past the cutoff -- so its `:rev` sub-filter (`:nop`) differs from its first parent's
(`:prefix=sub`). Reversing it must produce a *clean nested* tree (`sub/a`, no root-level `a`), not a
doubled root+nested tree, and re-filtering (checked with `--check-roundtrip`) must reproduce the
filtered merge. The `keep-trivial-merges` history flag is scoped to the `:rev` pass so the merge --
newly trivial in that pass -- is retained there, and the `:/sub` pass keeps it because it is already
trivial by then.

Before the fix the reverse takes the generic invert path, which collapses the per-commit cutoff and
doubles the tree; re-filtering then elides the merge and `--check-roundtrip` prints "Roundtrip
failed".

  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

Root-layout region: `a` lives at the repo root.

  $ echo 1 > a
  $ git add a
  $ git commit -q -m "root a=1"
  $ echo 2 > a
  $ git add a
  $ git commit -q -m "root a=2 (cutoff)"
  $ CUTOFF=$(git rev-parse HEAD)

Nested region: the project moves under `sub/`.

  $ git rm -q a 1> /dev/null
  $ mkdir sub
  $ echo 3 > sub/a
  $ git add sub
  $ git commit -q -m "nest a under sub"

  $ git log --graph --pretty=%s
  * nest a under sub
  * root a=2 (cutoff)
  * root a=1

Forward-filter to a continuous root-layout history. The root-layout commits are identity-filtered, so
the filtered history is linear and shares its trees with the original.

  $ FILTER=":~(history=\"keep-trivial-merges\")[:rev(<=${CUTOFF}:prefix=sub)]:/sub"
  $ josh-filter -s "${FILTER}" master --update refs/heads/filtered
  483d889ef3c32d1caadc9d2a43ac3d89042b49c4
  [3] :/sub
  [3] :~(
      history="keep-trivial-merges"
  )[
      :rev(<=e2607aa41afb761fc6b0a24dc6d8d16c7e30a978:prefix=sub)
  ]
  [6] reachable_roots
  [6] sequence_number

  $ git log refs/heads/filtered --graph --pretty=%s
  * nest a under sub
  * root a=2 (cutoff)
  * root a=1

Author a trivial merge on the filtered side: check out the commit derived from the root-layout cutoff
(`filtered~1`) and merge the filtered tip with `-s ours`, so the merge tree equals its first parent.

  $ git checkout -q -b fmerge refs/heads/filtered~1
  $ git merge -q -s ours --no-ff -m "Merge sync into root" refs/heads/filtered
  $ git log fmerge --graph --pretty=%s
  *   Merge sync into root
  |\  
  | * nest a under sub
  |/  
  * root a=2 (cutoff)
  * root a=1

The pre-reverse filtered tree of the merge (equal to its first parent, `root a=2`).

  $ git rev-parse fmerge^{tree}
  02439fb50f442f187d6db9a7b47287b9e3f5d49c

Reverse the merge back onto the original history and check the roundtrip. `--check-roundtrip` prints
the reconstructed OID (not "Roundtrip failed").

  $ josh-filter -s "${FILTER}" master --update refs/heads/fmerge --reverse --check-roundtrip
  47d6b2a094f6aa47dbcb60a36b1aebf068c529f4
  [4] :/sub
  [4] :~(
      history="keep-trivial-merges"
  )[
      :rev(<=e2607aa41afb761fc6b0a24dc6d8d16c7e30a978:prefix=sub)
  ]
  [10] reachable_roots
  [10] sequence_number

The reconstructed merge is present on master and has a clean nested tree: `sub/a`, no root-level `a`.

  $ git log master --graph --pretty=%s
  *   Merge sync into root
  |\  
  | * nest a under sub
  |/  
  * root a=2 (cutoff)
  * root a=1

  $ git ls-tree -r --name-only master
  sub/a
