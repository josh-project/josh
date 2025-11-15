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

Test josh clone with --keep-trivial-merges flag

  $ josh clone remote/libs :/sub1 libs --keep-trivial-merges
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote/libs
   * [new branch]      feature    -> refs/josh/remotes/origin/feature
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs
   * [new branch]      feature    -> origin/feature
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: libs

  $ cd libs

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ cd ..

Test josh remote add with --keep-trivial-merges flag

  $ mkdir test-repo
  $ cd test-repo
  $ git init -q
  $ josh remote add origin ${TESTTMP}/remote/libs :/sub1 --keep-trivial-merges
  Added remote 'origin' with filter ':/sub1'

  $ git config josh-remote.origin.filter
  :/sub1

  $ cd ..


