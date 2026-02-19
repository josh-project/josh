  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

  $ echo contents1 > file1
  $ git add .
  $ git commit -m "commit1" 1> /dev/null

  $ echo contents2 > file2
  $ git add .
  $ git commit -m "commit2" 1> /dev/null

  $ echo contents3 > file3
  $ git add .
  $ git commit -m "commit3" 1> /dev/null

  $ git log --oneline
  448e2ef commit3
  e86ec41 commit2
  1a7fc43 commit1

  $ COMMIT2=$(git rev-parse HEAD~1)
  $ josh-filter -s ":rev(<=$COMMIT2:prefix=x,<$COMMIT2:prefix=y)" --update refs/heads/filtered
  6df7a0701cf39b99b9f772d4625d3ba6518c787e
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x,<e86ec4160d1359883e642e6888645d28c3358012:prefix=y)
  [3] sequence_number
  $ git log --oneline refs/heads/filtered
  6df7a07 commit3
  6ca898f commit2
  5185af4 commit1
  $ git ls-tree -r --name-only refs/heads/filtered
  file1
  file2
  file3

  $ josh-filter -s ":rev(<=$COMMIT2:prefix=x)" --update refs/heads/filtered
  Warning: reference refs/heads/filtered wasn't updated
  6df7a0701cf39b99b9f772d4625d3ba6518c787e
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x)
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x,<e86ec4160d1359883e642e6888645d28c3358012:prefix=y)
  [3] sequence_number
  $ git ls-tree -r --name-only refs/heads/filtered
  file1
  file2
  file3
  $ git ls-tree -r --name-only refs/heads/filtered~1
  x/file1
  x/file2

  $ josh-filter -s ":rev(==$COMMIT2:prefix=x)" --update refs/heads/filtered
  4d6d863c9815e9109c26036b1743902bebd7cb22
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x)
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x,<e86ec4160d1359883e642e6888645d28c3358012:prefix=y)
  [3] :rev(==e86ec4160d1359883e642e6888645d28c3358012:prefix=x)
  [3] sequence_number
  $ git ls-tree -r --name-only refs/heads/filtered
  file1
  file2
  file3
  $ git ls-tree -r --name-only refs/heads/filtered~1
  x/file1
  x/file2
  $ git ls-tree -r --name-only refs/heads/filtered~2
  file1

  $ COMMIT3=$(git rev-parse HEAD)
  $ josh-filter -s ":rev(<$COMMIT3:prefix=x,_:prefix=y)" --update refs/heads/filtered
  17e0be6d0ffa8d86e806812df0a5406385592ae8
  [3] :rev(<448e2ef1935706609fc0fae23920f8ab414f8aa9:prefix=x,_:prefix=y)
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x)
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x,<e86ec4160d1359883e642e6888645d28c3358012:prefix=y)
  [3] :rev(==e86ec4160d1359883e642e6888645d28c3358012:prefix=x)
  [3] sequence_number
  $ git ls-tree -r --name-only refs/heads/filtered
  y/file1
  y/file2
  y/file3

  $ COMMIT1=$(git rev-parse HEAD~2)
  $ josh-filter -s ":rev(<=$COMMIT1:prefix=x,<=$COMMIT2:prefix=y)" --update refs/heads/filtered
  847106a280bfc26e84afb8f53ebd3435a51d4167
  [3] :rev(<448e2ef1935706609fc0fae23920f8ab414f8aa9:prefix=x,_:prefix=y)
  [3] :rev(<=1a7fc439ecd2dc3cb66663e07341ac9e3994abe5:prefix=x,<=e86ec4160d1359883e642e6888645d28c3358012:prefix=y)
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x)
  [3] :rev(<=e86ec4160d1359883e642e6888645d28c3358012:prefix=x,<e86ec4160d1359883e642e6888645d28c3358012:prefix=y)
  [3] :rev(==e86ec4160d1359883e642e6888645d28c3358012:prefix=x)
  [3] sequence_number
  $ git ls-tree -r --name-only refs/heads/filtered
  file1
  file2
  file3
