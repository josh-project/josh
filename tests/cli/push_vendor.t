Vendor a standalone repo into a subdirectory of a central repo using
`josh push --base`. The central repo has pre-existing history under `app/`.
A standalone (initially unrelated) vendor repo is brought in as a new
branch on the central repo, mounted under `libs/vendored/`.

This is the CLI equivalent of `josh-proxy`'s `git push -o base=...`
(see tests/proxy/push_subtree.t): the pushed branch should descend from
the unfiltered base, and its tree should be the base tree with the
vendored content overlaid at the filter mount point.

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

Standalone vendor repo with its own (initially unrelated) history

  $ git init -q --bare vendor-origin
  $ git init -q vendor-seed
  $ cd vendor-seed
  $ echo "lib v1" > lib.txt
  $ git add lib.txt
  $ git commit -q -m "vendor: initial lib"
  $ echo "lib v2" >> lib.txt
  $ git add lib.txt
  $ git commit -q -m "vendor: extend lib"
  $ git remote add origin ${TESTTMP}/vendor-origin
  $ git push -q origin master
  $ cd ${TESTTMP}

Clone the vendor repo with a normal `git clone` — this is the working
copy we'll vendor into central.

  $ git clone ${TESTTMP}/vendor-origin vendored
  Cloning into 'vendored'...
  done.
  $ cd vendored

Add the central repo as a josh remote that mounts our tree at
`libs/vendored`. The filter `:/libs/vendored` views central through its
`libs/vendored/` subdirectory; reverse-applying it on push places our
working-tree files there.

  $ josh remote add central ${TESTTMP}/central :/libs/vendored
  Added remote 'central' with filter ':/libs/vendored'

Fetch from central so `--base=master` can be resolved against
`refs/josh/remotes/central/master`. Central's master has no
`libs/vendored/` content, so the filtered side is empty and no local
tracking ref is updated — only the unfiltered base is populated.

  $ josh fetch --remote central
  From file://${TESTTMP}/central
   * [new branch]      master     -> refs/josh/remotes/central/master
  
  Fetched from remote: central

Push the vendor history into central as a new branch, using central
master as the unfiltered base for reverse filtering.

  $ josh push central HEAD:refs/heads/vendor-bring-in --base=master
  Pushing * to central/refs/heads/vendor-bring-in (glob)
  To file://${TESTTMP}/central
   * [new branch]      * -> vendor-bring-in (glob)
  
  Pushed 1 ref(s) to central

The pushed branch must descend from central master: central's two
commits plus the two vendor commits = 4 total.

  $ git --git-dir=${TESTTMP}/central log --oneline vendor-bring-in
  * vendor: extend lib (glob)
  * vendor: initial lib (glob)
  * central: extend app/main.txt (glob)
  * central: add app/main.txt (glob)

`master` must be an ancestor of `vendor-bring-in`.

  $ git --git-dir=${TESTTMP}/central merge-base --is-ancestor master vendor-bring-in && echo ok
  ok

The tip tree on `vendor-bring-in` must contain BOTH central's
pre-existing `app/main.txt` AND the vendored `libs/vendored/lib.txt`.

  $ git --git-dir=${TESTTMP}/central ls-tree -r --name-only vendor-bring-in
  app/main.txt
  libs/vendored/lib.txt

  $ git --git-dir=${TESTTMP}/central show vendor-bring-in:libs/vendored/lib.txt
  lib v1
  lib v2

  $ git --git-dir=${TESTTMP}/central show vendor-bring-in:app/main.txt
  central app v1
  central app v2

Round-trip: re-fetch central through our `:/libs/vendored` filter and
confirm the filtered view of `vendor-bring-in` is the original vendor
history — same commit OIDs, same tree, same file content.

  $ josh fetch --remote central
  From file://${TESTTMP}/central
   * [new branch]      vendor-bring-in -> refs/josh/remotes/central/vendor-bring-in
  
  From file://${TESTTMP}/vendored
   * [new branch]      vendor-bring-in -> central/vendor-bring-in
  
  Fetched from remote: central

Commit SHA equality: filtering `central/vendor-bring-in` must yield the
exact original vendor commit OIDs at every position in the history, not
just an equivalent tree.

  $ test "$(git rev-parse central/vendor-bring-in)" = "$(git rev-parse master)" && echo tip-equal
  tip-equal
  $ test "$(git rev-parse central/vendor-bring-in~1)" = "$(git rev-parse master~1)" && echo parent-equal
  parent-equal

  $ git log --format='%H %s' central/vendor-bring-in
  e5b0571b0818fd08eda800033b27fed2ba34cd2a vendor: extend lib
  e883d51680ac076fa712a01f3db69dd02c0bf7a0 vendor: initial lib
  $ git log --format='%H %s' master
  e5b0571b0818fd08eda800033b27fed2ba34cd2a vendor: extend lib
  e883d51680ac076fa712a01f3db69dd02c0bf7a0 vendor: initial lib

  $ git ls-tree -r --name-only central/vendor-bring-in
  lib.txt

  $ git show central/vendor-bring-in:lib.txt
  lib v1
  lib v2
