  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file1

  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ josh-filter master --update refs/heads/filter/master :/sub1
  $ git log --graph --pretty=%s filter/master
  * add file1

  $ git checkout filter/master
  Switched to branch 'filter/master'
  $ echo contents2 >> file1
  $ git commit -am "update file1 from filter" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'
  $ git checkout -b master_new
  Switched to a new branch 'master_new'
  $ echo contents1 >> unrelated_file
  $ git add unrelated_file
  $ git commit -am "unrelated change" 1> /dev/null
  $ git log --graph --pretty=%s
  * unrelated change
  * add file1

  $ josh-filter master_new --update refs/heads/filter/master_new :/sub1
  $ josh-filter master --reverse --update refs/heads/filter/master :/sub1
  $ git log --graph --pretty=%s master
  * update file1 from filter
  * add file1


