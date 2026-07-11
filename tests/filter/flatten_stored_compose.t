  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents2 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir st
  $ cat > st/a.josh <<EOF
  > wrap = :/sub1
  > EOF
  $ git add .
  $ git commit -m "add stored filter" 1> /dev/null

Chaining a Compose with a Stored filter must not error. The flatten optimizer
used to distribute the Chain over the Compose, producing a Compose whose
elements (Chains containing the non-invertible :+st/a) could not be inverted
by downstream optimization. The fix keeps the Chain intact when any duplicated
element is non-invertible.

  $ josh-filter -s ':[:/sub1,:/sub2]:+st/a'
  .{40} (re)
  [1] :+st/a
  [2] :[
      :/sub1
      :/sub2
  ]
  [5] reachable_roots
  [5] sequence_number

