  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo xcontents1 > sub1/xfile1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git rm sub1/file1
  rm 'sub1/file1'
  $ git commit -m "rm file1" 1> /dev/null
  $ echo contents2 >> sub1/xfile1
  $ git add sub1
  $ git commit -m "edit xfile1" 1> /dev/null
  $ echo contents2 > sub1/file1
  $ git add sub1
  $ git commit -m "edit file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null
