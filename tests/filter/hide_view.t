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

  $ git log --graph --pretty=%s
  * add file3
  * add file2
  * add file1

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file2
  `-- sub2
      `-- file3
  
  2 directories, 3 files

  $ josh-filter master:refs/josh/filter/master c=:hide=sub1
  $ git checkout josh/filter/master 2> /dev/null
  $ git log --graph --pretty=%s
  * add file3
  $ tree
  .
  `-- c
      `-- sub2
          `-- file3
  
  2 directories, 1 file

  $ josh-filter master:refs/josh/filter/master c=:hide=sub1/file2
  $ git checkout josh/filter/master 2> /dev/null
  $ git log --graph --pretty=%s
  * add file3
  * add file1
  $ tree
  .
  `-- c
      |-- sub1
      |   `-- file1
      `-- sub2
          `-- file3
  
  3 directories, 2 files

  $ josh-filter master:refs/josh/filter/master c=:hide=sub2/file3
  $ git checkout josh/filter/master 2> /dev/null
  $ git log --graph --pretty=%s
  * add file2
  * add file1
  $ tree
  .
  `-- c
      `-- sub1
          |-- file1
          `-- file2
  
  2 directories, 2 files
