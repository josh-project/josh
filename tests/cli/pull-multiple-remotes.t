  $ export TESTTMP=${PWD}


  $ cd ${TESTTMP}
  $ mkdir remote1 remote2
  $ cd remote1
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
  $ echo contents3 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ cd ${TESTTMP}/remote2
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo different1 > sub1/file1
  $ git add sub1
  $ git commit -m "add different file1" 1> /dev/null

  $ mkdir sub2
  $ echo different2 > sub2/file3
  $ git add sub2
  $ git commit -m "add different file3" 1> /dev/null

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

  $ josh --store libs.store clone ${TESTTMP}/remote1/libs libs --mount origin --filter :/sub1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added remote 'origin' at 'origin'
  Cloned worktree to ${TESTTMP}/libs

  $ cd libs

  $ git-tree-pretty .
  .
  ├── file1
  │   ┆  contents1
  └── file2
      ┆  contents2

  $ josh remote add remote2 ${TESTTMP}/remote2/libs 2>/dev/null

  $ josh changes pull --remote remote2
  Fetched from remote: remote2
  Error: the current branch tracks 'refs/remotes/origin/master', which does not belong to remote 'remote2'
  the current branch tracks 'refs/remotes/origin/master', which does not belong to remote 'remote2'
  [1]

  $ git-tree-pretty .
  .
  ├── file1
  │   ┆  contents1
  └── file2
      ┆  contents2

  $ josh changes pull
  Fetched from remote: origin
  Already up to date.

  $ git-tree-pretty .
  .
  ├── file1
  │   ┆  contents1
  └── file2
      ┆  contents2
