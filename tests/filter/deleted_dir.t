If a directory gets deleted then the last commit
in that subtree repo should have an empty tree

  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master
  21a904a6f350cb1f8ea4dc6fe9bd4e3b4cc4840b
  [2] :/sub1
  [2] :prefix=c
  [4] sequence_number

  $ git log refs/josh/filter/master --graph --pretty=%s
  * add file2
  * add file1

  $ git ls-tree --name-only -r refs/josh/filter/master 
  c/file1
  c/file2

  $ git rm -r sub1
  rm 'sub1/file1'
  rm 'sub1/file2'
  $ git commit -m "rm sub1" 1> /dev/null

  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master
  1e8394fa1057f9c14155ea4f612320544ec3510d
  [3] :/sub1
  [3] :prefix=c
  [6] sequence_number

  $ git log refs/josh/filter/master --graph --pretty=%s
  * rm sub1
  * add file2
  * add file1

  $ git ls-tree --name-only -r refs/josh/filter/master 
