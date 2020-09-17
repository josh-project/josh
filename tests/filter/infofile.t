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

  $ mkdir sub2
  $ echo contents1 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ echo contents1 > unrelated
  $ git add .
  $ git commit -m "unrelated" 1> /dev/null

  $ josh-filter --infofile master:refs/josh/filter/master c=:/sub1
  $ git log --graph --pretty=%s josh/filter/master
  * add file2
  * add file1

  $ josh-filter master:refs/josh/filter/master c=:/sub1
  $ git log --graph --pretty=%s josh/filter/master
  * add file2
  * add file1

  $ josh-filter --infofile master:refs/josh/filter/master c=:/sub2
  $ git log --graph --pretty=%s josh/filter/master
  * add file3

  $ echo contents2 > sub1/file5
  $ git add sub1
  $ git commit -m "add file5" 1> /dev/null

  $ josh-filter --infofile master:refs/josh/filter/master c=:/sub2
  $ git log --graph --pretty=%s josh/filter/master
  * add file3
