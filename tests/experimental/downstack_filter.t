  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

  $ echo base > base
  $ git add .
  $ git commit -m "base" 1> /dev/null
  $ BASE=$(git rev-parse HEAD)

  $ echo a > file_a
  $ git add .
  $ git commit -m "add file_a" 1> /dev/null

  $ echo b > file_b
  $ git add .
  $ git commit -m "add file_b" 1> /dev/null

  $ echo b2 > file_b
  $ git add .
  $ git commit -m "modify file_b" 1> /dev/null

  $ git log --pretty=%s
  modify file_b
  add file_b
  add file_a
  base

The downstack filter rebuilds the stack from the tip onto $BASE,
dropping intermediate commits whose paths are disjoint from the tip's changes.
The tip only touches file_b, so "add file_a" is dropped but "add file_b" is kept.

  $ josh-filter -s ":_=$BASE" --update refs/heads/filtered 1> /dev/null
  $ git log refs/heads/filtered --pretty=%s
  modify file_b
  add file_b
  base

The resulting tip has the expected tree:

  $ git show refs/heads/filtered:file_b
  b2
  $ git ls-tree --name-only refs/heads/filtered
  base
  file_b

Roundtrip: filter spec is preserved.

  $ josh-filter -p ":_=$BASE"
  :_=[0-9a-f]{40} (re)

Error: non-descendant base.

  $ UNRELATED=$(git commit-tree $EMPTY_TREE -m "unrelated")
  $ josh-filter -s ":_=$UNRELATED" --update refs/heads/foo 2>&1 | grep -i "not a descendant"
  ERROR: change [0-9a-f]+ is not a descendant of base [0-9a-f]+ (re)
