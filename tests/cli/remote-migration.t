  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ mkdir remote
  $ cd remote
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add files" 1> /dev/null

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

Test migration from legacy git config to new file format

  $ mkdir test-repo
  $ cd test-repo
  $ git init -q

  $ git config josh-remote.origin.url "file://${TESTTMP}/remote/libs"
  $ git config josh-remote.origin.filter ":/sub1"
  $ git config josh-remote.origin.fetch "+refs/heads/*:refs/josh/remotes/origin/*"

  $ git config josh-remote.origin.url | sed "s|file://.*/remote/libs|file://\${TESTTMP}/remote/libs|"
  file://${TESTTMP}/remote/libs
  $ git config josh-remote.origin.filter
  :/sub1

  $ josh filter origin
  Applying filter ':/sub1' to remote 'origin'
  Error: No remote references found
  No remote references found
  [1]

  $ cat .git/josh/remotes/origin.josh | sed "s|file://.*/remote/libs|file://\${TESTTMP}/remote/libs|"
  :~(
      fetch="+refs/heads/*:refs/josh/remotes/origin/*"
      url="file://${TESTTMP}/remote/libs"
  )[
      :/sub1
  ]

  $ cd ..
