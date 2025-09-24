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

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

  $ josh clone ${TESTTMP}/remote/libs :/sub1 libs
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  From $TESTTMP/remote/libs
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/libs
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

  $ echo newfile > newfile
  $ git add newfile
  $ git commit -m "add newfile" 1> /dev/null

  $ cd ${TESTTMP}/remote/libs
  $ echo remote_newfile > sub1/remote_newfile
  $ git add sub1
  $ git commit -m "add remote_newfile" 1> /dev/null

  $ cd ${TESTTMP}/libs

  $ josh fetch
  From $TESTTMP/remote/libs
     81b10fb..0956fb2  master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/libs
     d8388f5..61e377b  master     -> origin/master
  
  Fetched from remote: origin

  $ tree
  .
  |-- file1
  |-- file2
  `-- newfile
  
  1 directory, 3 files

  $ git log --oneline
  6a6f932 add newfile
  d8388f5 add file2
  0b4cf6c add file1

  $ git log --oneline origin/master
  61e377b add remote_newfile
  d8388f5 add file2
  0b4cf6c add file1

  $ git checkout origin/master
  Note: switching to 'origin/master'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at 61e377b add remote_newfile

  $ tree
  .
  |-- file1
  |-- file2
  `-- remote_newfile
  
  1 directory, 3 files
