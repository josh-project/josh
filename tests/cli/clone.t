  $ export TESTTMP=${PWD}

  $ export JOSH_STORE=${TESTTMP}/store


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

  $ josh clone remote/libs libs --filter :/sub1 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added remote 'libs' at 'libs'
  Cloned worktree to ${TESTTMP}/libs

  $ cd libs


  $ tree .git/refs
  .git/refs
  |-- heads
  |   `-- master
  |-- remotes
  |   `-- libs
  |       |-- HEAD
  |       |-- feature
  |       `-- master
  `-- tags
  
  5 directories, 4 files

  $ git-tree-pretty .
  .
  ├── file1
  │   ┆  contents1
  └── file2
      ┆  contents2

  $ git checkout feature
  branch 'feature' set up to track 'libs/feature'.
  Switched to a new branch 'feature'

  $ git-tree-pretty .
  .
  ├── file1
  │   ┆  contents1
  └── file2
      ┆  contents2

The same stored remote backs another projection without another remote add.

  $ cd ${TESTTMP}
  $ josh clone remote/libs libs-root 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Cloned worktree to ${TESTTMP}/libs-root
  $ (cd libs-root && git-tree-pretty .)
  .
  \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 sub1/ (escaped)
  │   ├── file1
  │   │   ┆  contents1
  │   └── file2
  │       ┆  contents2
  \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 sub2/ (escaped)
      └── file3
          ┆  contents1

Worktree paths are relative to the invocation directory, independently of the store.

  $ mkdir ${TESTTMP}/elsewhere
  $ cd ${TESTTMP}/elsewhere
  $ josh worktree add ../external :/libs/sub2 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added worktree at ${TESTTMP}/external
  $ rm -rf ../external
  $ josh worktree remove ../external


