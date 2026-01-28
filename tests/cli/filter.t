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
  Added remote 'origin' with filter ':/sub1'
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

  $ cat .git/josh/remotes/origin.josh | sed "s|file://.*/remote/libs|file://\${TESTTMP}/remote/libs|"
  :~(
      fetch="+refs/heads/*:refs/josh/remotes/origin/*"
      url="file://${TESTTMP}/remote/libs"
  )[
      :/sub1
  ] (no-eol)

  $ josh filter origin
  Applying filter ':/sub1' to remote 'origin'
  Applied filter to remote: origin

  $ git log --oneline
  1432d42 add files

  $ cd ..

Test josh filter with non-existent remote

  $ mkdir test-repo
  $ cd test-repo
  $ git init -q

  $ josh filter nonexistent 2>&1 | sed "s|Failed to read remote config file: .*|Failed to read remote config file: *|"
  Error: Failed to read remote config for 'nonexistent'
  Failed to read remote config for 'nonexistent'
  Remote 'nonexistent' not found in new format (.git/josh/remotes/nonexistent.josh) or legacy git config (josh-remote.nonexistent)

  $ cd ..


