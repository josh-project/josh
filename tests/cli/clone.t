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

  $ josh clone remote/libs :/sub1 libs
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  Fetched from remote: origin
  Pulled from remote: origin
  Cloned repository to: libs

  $ cd libs

  $ tree .git/refs
  .git/refs
  |-- heads
  |   |-- feature
  |   `-- master
  |-- josh
  |   `-- remotes
  |       `-- origin
  |           |-- feature
  |           `-- master
  |-- remotes
  |   `-- origin
  |       |-- HEAD
  |       |-- feature
  |       `-- master
  `-- tags
  
  8 directories, 7 files

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ git checkout feature
  Switched to branch 'feature'

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files


