  $ export TESTTMP=${PWD}
  $ git init -q remote
  $ cd remote
  $ mkdir sub1
  $ echo content > sub1/file
  $ git add .
  $ git commit -q -m initial
  $ git branch feature
  $ cd ..

An explicit --ref fetches only the requested branch.

  $ git init -q repo
  $ cd repo
  $ git commit -q --allow-empty -m initial
  $ josh remote add origin ${TESTTMP}/remote :/sub1
  Added remote 'origin' with filter ':/sub1'
  $ josh fetch --ref feature 2>/dev/null
  $ git for-each-ref --format='%(refname)' refs/josh/remotes/origin refs/remotes/origin
  refs/josh/remotes/origin/feature
  refs/remotes/origin/feature

Fetching the default branch later adds it without disturbing the first branch.

  $ josh fetch --ref master 2>/dev/null
  $ git for-each-ref --format='%(refname)' refs/josh/remotes/origin refs/remotes/origin
  refs/josh/remotes/origin/feature
  refs/josh/remotes/origin/master
  refs/remotes/origin/HEAD
  refs/remotes/origin/feature
  refs/remotes/origin/master
