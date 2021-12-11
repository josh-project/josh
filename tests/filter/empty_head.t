  $ export TESTTMP=${PWD}

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

  $ mkdir sub2
  $ echo contents1 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ josh-filter -s :/sub1 master --update refs/josh/filter/master
  [2] :/sub1
  $ git log --graph --pretty=%s josh/filter/master
  * add file2
  * add file1

  $ josh-filter -s :/sub2 master --update refs/josh/filter/master
  [2] :/sub1
  [2] :/sub2
  $ git log --graph --pretty=%s josh/filter/master
  * add file3

  $ echo contents2 > sub1/file5
  $ git add sub1
  $ git commit -m "add file5" 1> /dev/null

  $ josh-filter -s :/sub2 master --update refs/josh/filter/master
  Warning: reference refs/josh/filter/master wasn't updated
  [2] :/sub1
  [2] :/sub2
  $ git log --graph --pretty=%s josh/filter/master
  * add file3
