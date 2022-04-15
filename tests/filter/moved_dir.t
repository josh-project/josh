If a directory gets moved to another part of the tree the last commit
in that subtree repo should have an empty tree

  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q libs 1>/dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master
  [2] :/sub1
  [2] :prefix=c

  $ git log refs/josh/filter/master --graph --pretty=%s
  * add file2
  * add file1

  $ git ls-tree --name-only -r refs/josh/filter/master 
  c/file1
  c/file2

  $ git mv sub1 sub1_new
  $ git commit -m "mv sub1" 1>/dev/null

  $ git ls-tree --name-only -r master
  sub1_new/file1
  sub1_new/file2

  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master
  [3] :/sub1
  [3] :prefix=c

  $ git log refs/josh/filter/master --graph --pretty=%s
  * mv sub1
  * add file2
  * add file1

  $ git ls-tree --name-only -r refs/josh/filter/master 

  $ echo contents2 > unrelated_file
  $ git add unrelated_file
  $ git commit -m "add unrelated_file" 1> /dev/null
  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master2
  [3] :/sub1
  [3] :prefix=c
  $ git log refs/josh/filter/master2 --graph --pretty=%s
  * mv sub1
  * add file2
  * add file1
