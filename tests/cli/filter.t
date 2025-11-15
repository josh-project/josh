  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ mkdir remote
  $ cd remote
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add files" 1> /dev/null

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

Test josh filter command - apply filtering without fetching

  $ josh clone ${TESTTMP}/remote/libs :/sub1 filtered-repo
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  From file://${TESTTMP}/remote/libs
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/filtered-repo
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/filtered-repo

  $ cd filtered-repo

  $ ls
  file1
  file2

  $ git log --oneline
  1432d42 add files

  $ git config josh-remote.origin.filter
  :/sub1:prune=trivial-merge

  $ josh filter origin
  Applying filter ':/sub1:prune=trivial-merge' to remote 'origin'
  Applied filter to remote: origin

  $ git log --oneline
  1432d42 add files

  $ cd ..

Test josh filter with non-existent remote

  $ mkdir test-repo
  $ cd test-repo
  $ git init -q

  $ josh filter nonexistent
  Error: No filter configured for remote 'nonexistent'
  No filter configured for remote 'nonexistent'
  config value 'josh-remote.nonexistent.filter' was not found; class=Config (7); code=NotFound (-3)
  [1]

  $ cd ..


