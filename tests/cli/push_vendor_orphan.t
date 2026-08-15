Vendor a standalone repo into a subdirectory of a central repo *without*
`--base`, keeping the vendored history as a disconnected (orphan) branch.

This is the workflow from issue #2205: the pushed branch should carry the
full vendored history (one commit per original commit, not a single squash)
and must NOT be reparented onto central's history — it stays unconnected so
it can be merged into the main branch separately.

Contrast with tests/cli/push_vendor.t, which uses `--base=master` to make the
vendored history descend from central master.

Setup

  $ export TESTTMP=${PWD}

Central repo (bare) with pre-existing history under app/

  $ git init -q --bare central
  $ git init -q central-seed
  $ cd central-seed
  $ mkdir -p app
  $ echo "central app v1" > app/main.txt
  $ git add app
  $ git commit -q -m "central: add app/main.txt"
  $ echo "central app v2" >> app/main.txt
  $ git add app
  $ git commit -q -m "central: extend app/main.txt"
  $ git remote add origin ${TESTTMP}/central
  $ git push -q origin master
  $ cd ${TESTTMP}

Standalone vendor repo with its own unrelated history (three commits).

  $ git init -q vendored
  $ cd vendored
  $ echo "lib v1" > lib.txt
  $ git add lib.txt
  $ git commit -q -m "vendor: initial lib"
  $ echo "lib v2" >> lib.txt
  $ git add lib.txt
  $ git commit -q -m "vendor: extend lib"
  $ echo "lib v3" >> lib.txt
  $ git add lib.txt
  $ git commit -q -m "vendor: extend lib again"

Add central as a josh remote mounting our tree at `libs/vendored`.

  $ josh remote add central ${TESTTMP}/central :/libs/vendored
  Added remote 'central' with filter ':/libs/vendored'

Push the vendor history into central as a new branch, WITHOUT `--base`.

  $ josh push central HEAD:refs/heads/vendored-in
  Pushing * to central/refs/heads/vendored-in (glob)
  To file://${TESTTMP}/central
   * [new branch]      * -> vendored-in (glob)
  
  Pushed 1 ref(s) to central


All three vendor commits must be present — not collapsed into one.

  $ git --git-dir=${TESTTMP}/central log --oneline vendored-in
  * vendor: extend lib again (glob)
  * vendor: extend lib (glob)
  * vendor: initial lib (glob)

The branch must be an orphan: central `master` is NOT an ancestor of it,
and the oldest commit has no parents.

  $ git --git-dir=${TESTTMP}/central merge-base --is-ancestor master vendored-in && echo connected || echo unconnected
  unconnected
  $ git --git-dir=${TESTTMP}/central rev-list --max-parents=0 vendored-in | wc -l | tr -d ' '
  1

The tree carries only the mounted vendored content (no central `app/`).

  $ git --git-dir=${TESTTMP}/central ls-tree -r --name-only vendored-in
  libs/vendored/lib.txt
  $ git --git-dir=${TESTTMP}/central show vendored-in:libs/vendored/lib.txt
  lib v1
  lib v2
  lib v3

Round-trip: filtering the pushed branch back through `:/libs/vendored`
reproduces the original vendor commits exactly — same OIDs, lossless.

  $ josh fetch --remote central
  new branch vendored-in
  Fetched from remote: central



  $ test "$(git rev-parse central/vendored-in)" = "$(git rev-parse master)" && echo tip-equal
  tip-equal
  $ git log --format='%s' central/vendored-in
  vendor: extend lib again
  vendor: extend lib
  vendor: initial lib
