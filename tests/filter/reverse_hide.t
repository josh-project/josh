  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ josh-filter -s :hide=sub2 master --update refs/heads/hidden
  [2 -> 1] :hide=sub2
  $ git checkout hidden 1> /dev/null
  Switched to branch 'hidden'
  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file
  $ git log --graph --pretty=%s
  * add file1

  $ echo contents3 > sub1/file3
  $ git add sub1/file3
  $ git commit -m "add sub1/file3" 1> /dev/null

  $ josh-filter -s :hide=sub2 --reverse master --update refs/heads/hidden
  [2 -> 1] :hide=sub2

  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file3
  `-- sub2
      `-- file2
  
  2 directories, 3 files


  $ cat sub1/file3
  contents3

  $ git log --graph --pretty=%s
  * add sub1/file3
  * add file2
  * add file1
