  $ export TESTTMP=${PWD}
  $ git init -q upstream
  $ cd upstream
  $ git commit -q --allow-empty -m initial
  $ cd ..
  $ git init -q repo
  $ cd repo
  $ git commit -q --allow-empty -m initial

Josh remotes can be listed and inspected.

  $ josh remote add origin ../upstream :/src
  Added remote 'origin' with filter ':/src'

  $ josh remote list
  origin	:/src	file://${TESTTMP}/upstream

  $ josh remote show origin
  Remote: origin
  URL: file://${TESTTMP}/upstream
  Filter: :/src
  Fetch: +refs/heads/*:refs/josh/remotes/origin/*
  Forge: none

Status summarizes repository state and Josh remotes.

  $ josh status | sed "s|${TESTTMP}|\${TESTTMP}|"
  Repository: ${TESTTMP}/repo/
  Branch: master
  Working tree: clean
  Josh remotes:
    origin  :/src  file://${TESTTMP}/upstream

Filters can be updated without editing private configuration files.

  $ josh remote set-filter origin :/
  Set filter for remote 'origin' to ':/'
  $ josh remote show origin | grep '^Filter:'
  Filter: :/

Removal supports dry-run and cleans both Josh and Git configuration.

  $ josh remote remove origin --dry-run
  Would remove Josh remote 'origin'
  $ git remote
  origin

  $ josh remote remove origin
  Removed Josh remote 'origin'
  $ josh remote list
  No Josh remotes configured.
  $ git remote

SCP-style SSH URLs are accepted without treating them as local paths.

  $ josh remote add ssh git@example.com:org/repo.git :/
  Added remote 'ssh' with filter ':/'
  $ josh remote show ssh | grep '^URL:'
  URL: git@example.com:org/repo.git
  $ josh remote remove ssh
  Removed Josh remote 'ssh'
