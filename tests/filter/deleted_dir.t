If a directory gets deleted then the last commit
in that subtree repo should have an empty tree

  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master
  [2 -> 2] :/sub1
  [2 -> 2] :prefix=c

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
  [3 -> 3] :/sub1
  [3 -> 3] :prefix=c

  $ git log refs/josh/filter/master --graph --pretty=%s
  * rm sub1
  * add file2
  * add file1

  $ git ls-tree --name-only -r refs/josh/filter/master 
