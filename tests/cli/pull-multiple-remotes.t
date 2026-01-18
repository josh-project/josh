  $ export TESTTMP=${PWD}


  $ cd ${TESTTMP}
  $ mkdir remote1 remote2
  $ cd remote1
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
  $ echo contents3 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ cd ${TESTTMP}/remote2
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo different1 > sub1/file1
  $ git add sub1
  $ git commit -m "add different file1" 1> /dev/null

  $ mkdir sub2
  $ echo different2 > sub2/file3
  $ git add sub2
  $ git commit -m "add different file3" 1> /dev/null

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

  $ josh clone ${TESTTMP}/remote1/libs :/sub1 libs
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote1/libs
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/libs

  $ cd libs

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ josh remote add remote2 ${TESTTMP}/remote2/libs :/sub2
  Added remote 'remote2' with filter ':/sub2'

  $ josh pull --remote remote2
  From file://${TESTTMP}/remote2/libs
   * [new branch]      master     -> refs/josh/remotes/remote2/master
  
  From file://${TESTTMP}/libs
   * [new branch]      master     -> remote2/master
  
  Fetched from remote: remote2
  You asked to pull from the remote 'remote2', but did not specify
  a branch. Because this is not the default configured remote
  for your current branch, you must specify a branch on the command line.
  
  Error: git pull failed
  git pull failed
  Command exited with code 1: git pull remote2
  [1]

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ josh pull
  Fetched from remote: origin
  Pulled from remote: origin

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files
