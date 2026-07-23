Two directories with identical contents have the same subtree oid. A pattern filter matches full
paths, so the filtered result of such a subtree depends on where it sits: the glob cache must not
serve the result computed for "a/" when walking "b/". Prior to the state-keyed glob cache, the
entry for the shared oid aliased across paths and "b/f.txt" was wrongly kept, depending on walk
order within the process.

  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir a b
  $ echo same > a/f.txt
  $ echo same > b/f.txt
  $ git add .
  $ git commit -m "add identical files" 1> /dev/null

The subtrees of a/ and b/ are identical:

  $ test "$(git rev-parse HEAD:a)" = "$(git rev-parse HEAD:b)"

  $ josh-filter -s "::a/*.txt" master --update refs/heads/filtered
  ac628907bfee2d475634285db2fd0909ac42d7d6
  [1] ::a/*.txt
  [1] reachable_roots
  [1] sequence_number

Only a/f.txt may survive; b/f.txt must not leak in via the shared subtree oid:

  $ git ls-tree -r --name-only refs/heads/filtered
  a/f.txt
