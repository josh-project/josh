The test forge is a test-suite-only forge variant: `--forge test` round-trips
through remote and link config like any other forge, but it is never guessed
from a URL.

  $ export TESTTMP=${PWD}

Set up a remote repository and a work repo.

  $ git init -q remote
  $ cd remote
  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -q -m "add file1"
  $ cd ${TESTTMP}

  $ git init -q work
  $ cd work

A remote records the test forge as a meta key in its config file.

  $ josh remote add origin ${TESTTMP}/remote :/sub1 --forge test
  Added remote 'origin' with filter ':/sub1'

  $ cat .git/josh/remotes/origin.josh | sed "s|file://.*/remote|file://\${TESTTMP}/remote|"
  :~(
      fetch="+refs/heads/*:refs/josh/remotes/origin/*"
      forge="test"
      url="file://${TESTTMP}/remote"
  )[
      :/sub1
  ]

A link records the test forge the same way (links are experimental).

  $ export JOSH_EXPERIMENTAL_FEATURES=1
  $ josh link add ${TESTTMP}/remote :/sub1 --forge test | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added link 'remote': ${TESTTMP}/remote refs/heads/master :/sub1

  $ git show refs/josh/links/remote:link.josh | sed "s|${TESTTMP}|\${TESTTMP}|g"
  :~(
      forge="test"
      tracked-ref="refs/heads/master"
      url="${TESTTMP}/remote"
  )[
      :/sub1
  ]

  $ josh link list | sed "s|${TESTTMP}|\${TESTTMP}|g"
  remote\t${TESTTMP}/remote\trefs/heads/master\ttest\t:/sub1 (escaped)
