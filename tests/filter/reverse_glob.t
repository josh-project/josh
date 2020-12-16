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

  $ josh-filter ":~glob=sub1/file*" master --update refs/heads/hidden
  $ git checkout hidden 1> /dev/null
  Switched to branch 'hidden'
  $ tree
  .
  |-- sub1
  |   `-- xfile1
  `-- sub2
      `-- file2
  
  2 directories, 2 files
  $ git log --graph --pretty=%s
  * add file2
  * edit xfile1
  * add file1

  $ echo contents3 > sub1/file3
  $ echo contents4 > sub2/file4
  $ git add .
  $ git commit -m "add sub1/file3, sub2/file4" 1> /dev/null

  $ josh-filter ":~glob=sub1/file*" --reverse master --update refs/heads/hidden

  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- xfile1
  `-- sub2
      |-- file2
      `-- file4
  
  2 directories, 4 files

  $ git log --graph --pretty=%s
  * add sub1/file3, sub2/file4
  * add file2
  * edit file1
  * edit xfile1
  * rm file1
  * add file1
