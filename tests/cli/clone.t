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
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote/libs
   * [new branch]      feature    -> refs/josh/remotes/origin/feature
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs
   * [new branch]      feature    -> origin/feature
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/libs

  $ cd libs


  $ tree .git/refs
  .git/refs
  |-- heads
  |   `-- master
  |-- josh
  |   |-- cache
  |   |   `-- 26
  |   |       `-- 0
  |   |           `-- bf567e0faf634a663d6cef48145a035e1974ab1d
  |   `-- remotes
  |       `-- origin
  |           |-- feature
  |           `-- master
  |-- namespaces
  |   `-- josh-origin
  |       |-- HEAD
  |       `-- refs
  |           `-- heads
  |               |-- feature
  |               `-- master
  |-- remotes
  |   `-- origin
  |       |-- HEAD
  |       |-- feature
  |       `-- master
  `-- tags
  
  15 directories, 10 files

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ git checkout feature
  branch 'feature' set up to track 'origin/feature'.
  Switched to a new branch 'feature'

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files


