  $ export TESTTMP=${PWD}


  $ cd ${TESTTMP}
  $ mkdir remote
  $ cd remote
  $ git init -q libs 1> /dev/null
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
  $ git branch feature

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

  $ josh clone ${TESTTMP}/remote/libs:/sub1
  Successfully added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin
  Successfully cloned repository to: libs

  $ cd libs

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ cd ${TESTTMP}/remote/libs

  $ echo new_content > sub1/newfile
  $ git add sub1
  $ git commit -m "add newfile" 1> /dev/null

  $ cd ${TESTTMP}/libs

  $ josh pull
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin

  $ tree
  .
  |-- file1
  |-- file2
  `-- newfile
  
  1 directory, 3 files

  $ git checkout feature
  Switched to branch 'feature'

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

