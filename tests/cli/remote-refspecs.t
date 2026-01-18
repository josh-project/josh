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

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

Test that josh remote add sets up proper refspecs

  $ mkdir test-repo
  $ cd test-repo
  $ git init -q
  $ josh remote add origin ${TESTTMP}/remote/libs :/sub1
  Added remote 'origin' with filter ':/sub1'

  $ git config --get-all remote.origin.fetch
  +refs/heads/*:refs/remotes/origin/*

  $ git config josh-remote.origin.filter
  :/sub1

  $ cd ..

Test that josh clone also sets up proper refspecs

  $ josh clone ${TESTTMP}/remote/libs :/sub1 cloned-repo
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote/libs
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/cloned-repo
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/cloned-repo

  $ cd cloned-repo

  $ git config --get-all remote.origin.fetch
  +refs/heads/*:refs/remotes/origin/*

  $ git config josh-remote.origin.filter
  :/sub1

  $ cd ..


