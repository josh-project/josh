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

  $ josh clone ${TESTTMP}/remote/libs :/sub1 libs
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  From $TESTTMP/remote/libs
   * [new branch]      feature    -> refs/josh/remotes/origin/feature
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/libs
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

  $ cd ${TESTTMP}/remote/libs

  $ echo new_content > sub1/newfile
  $ git add sub1
  $ git commit -m "add newfile" 1> /dev/null

  $ cd ${TESTTMP}/libs

  $ josh pull
  From $TESTTMP/remote/libs
     667a912..2c470be  master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/libs
     d8388f5..0974639  master     -> origin/master
  
  Fetched from remote: origin
  Pulled from remote: origin

  $ tree
  .
  |-- file1
  |-- file2
  `-- newfile
  
  1 directory, 3 files

  $ git checkout feature
  branch 'feature' set up to track 'origin/feature'.
  Switched to a new branch 'feature'

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

