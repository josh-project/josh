A scoped worktree can push an independently rooted branch through its mount
without collapsing or reparenting history.

  $ export TESTTMP=${PWD}

  $ git init -q --bare vendor
  $ git init -q seed
  $ echo "lib v1" > seed/lib.txt
  $ git -C seed add lib.txt
  $ git -C seed commit -q -m "vendor: initial lib"
  $ echo "lib v2" >> seed/lib.txt
  $ git -C seed commit -q -am "vendor: extend lib"
  $ echo "lib v3" >> seed/lib.txt
  $ git -C seed commit -q -am "vendor: extend lib again"
  $ git -C seed remote add origin ${TESTTMP}/vendor
  $ git -C seed push -q origin master

  $ josh --store store init >/dev/null
  $ cd store
  $ josh remote add libs/vendored ${TESTTMP}/vendor 2>/dev/null
  $ josh worktree add work :/libs/vendored 2>/dev/null >/dev/null
  $ cd work
  $ original=$(git rev-parse HEAD)
  $ josh push libs%2Fvendored HEAD:refs/heads/vendored-in >/dev/null 2>&1

  $ git --git-dir=${TESTTMP}/vendor log --format=%s vendored-in
  vendor: extend lib again
  vendor: extend lib
  vendor: initial lib
  $ git --git-dir=${TESTTMP}/vendor rev-list --max-parents=0 vendored-in | wc -l | tr -d ' '
  1
  $ (cd ${TESTTMP}/vendor && git-tree-pretty vendored-in)
  .
  └── lib.txt
      ┆  lib v1
      ┆  lib v2
      ┆  lib v3
  $ test "$(git --git-dir=${TESTTMP}/vendor rev-parse vendored-in)" = "$original" && echo tip-equal
  tip-equal
