Wrap a vendored push in an explicit merge commit via `josh push --merge`,
mirroring `josh-proxy`'s `git push -o merge`. The pushed branch's tip is
a merge commit with two parents: the central base, and the
reverse-filtered tip of the local commits.

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

Standalone vendor repo

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

Clone the vendor with vanilla `git clone`, wire up central, fetch the base.

  $ git clone -q ${TESTTMP}/vendor-origin vendored
  $ cd vendored
  $ josh remote add central ${TESTTMP}/central :/libs/vendored
  Added remote 'central' with filter ':/libs/vendored'
  $ josh fetch --remote central
  From file://${TESTTMP}/central
   * [new branch]      master     -> refs/josh/remotes/central/master
  
  Fetched from remote: central

Push with --merge --base=master. The reverse-filtered vendor commits are
wrapped in a merge commit on top of central master.

  $ josh push central HEAD:refs/heads/vendor-bring-in --base=master --merge
  Pushing * to central/refs/heads/vendor-bring-in (glob)
  To file://${TESTTMP}/central
   * [new branch]      * -> vendor-bring-in (glob)
  
  Pushed 1 ref(s) to central

The pushed tip is a merge commit: rev-list with --parents prints three
OIDs (the tip and its two parents).

  $ git --git-dir=${TESTTMP}/central rev-list --parents -n 1 vendor-bring-in | wc -w | tr -d ' '
  3

Merge commit subject identifies the source filter.

  $ git --git-dir=${TESTTMP}/central log -1 --format='%s' vendor-bring-in
  Merge from :/libs/vendored

Full graph: a merge node with two parent lines feeding back into master.

  $ git --git-dir=${TESTTMP}/central log --oneline --graph vendor-bring-in
  *   9e7fd16 Merge from :/libs/vendored
  |\  
  | * e37e256 vendor: extend lib
  | * 641e856 vendor: initial lib
  |/  
  * 12640c6 central: extend app/main.txt
  * 1f1a5d1 central: add app/main.txt

Master is an ancestor of vendor-bring-in.

  $ git --git-dir=${TESTTMP}/central merge-base --is-ancestor master vendor-bring-in && echo ok
  ok

The merge tip tree contains BOTH central's pre-existing `app/main.txt`
AND the vendored `libs/vendored/lib.txt`, with original content.

  $ git --git-dir=${TESTTMP}/central ls-tree -r --name-only vendor-bring-in
  app/main.txt
  libs/vendored/lib.txt

  $ git --git-dir=${TESTTMP}/central show vendor-bring-in:app/main.txt
  central app v1
  central app v2

  $ git --git-dir=${TESTTMP}/central show vendor-bring-in:libs/vendored/lib.txt
  lib v1
  lib v2

Reject path: `--merge` against a non-existent destination ref with no
`--base` has no commit to merge against and must fail with a clear error.

  $ josh push central HEAD:refs/heads/other --merge
  Error: --merge requires --base=<ref> or an existing destination ref
  [1]
