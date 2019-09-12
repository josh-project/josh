  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs &>/dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" &> /dev/null

  $ git checkout -b foo
  Switched to a new branch 'foo'

  $ mkdir sub2
  $ echo contents1 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" &> /dev/null

  $ cd ${TESTTMP}
  $ git init apps &>/dev/null
  $ cd apps

  $ git remote add libs ${TESTTMP}/libs
  $ git fetch --all
  Fetching libs
  From * (glob)
   * [new branch]      foo        -> libs/foo
   * [new branch]      master     -> libs/master

  $ git commit -m "initial" --allow-empty &> /dev/null

  $ josh-filter --infofile libs/master c=:/sub1
  $ git log --graph --pretty=%s josh/filter/libs/master
  * add file2
  * add file1
  $ josh-filter --infofile libs/foo a/b=:/sub2
  $ git log --graph --pretty=%s josh/filter/libs/foo
  * add file3

  $ git branch -a
  * master
    remotes/libs/foo
    remotes/libs/master

  $ git show-ref | grep josh/filter | sed 's/.* //'
  refs/josh/filter/libs/foo
  refs/josh/filter/libs/master

  $ tree .git/refs/
  .git/refs/
  |-- heads
  |   `-- master
  |-- josh
  |   `-- filter
  |       `-- libs
  |           |-- foo
  |           `-- master
  |-- remotes
  |   `-- libs
  |       |-- foo
  |       `-- master
  `-- tags
  
  7 directories, 5 files

  $ git read-tree HEAD josh/filter/libs/master josh/filter/libs/foo
  $ git commit -m "sync"
  [master *] sync (glob)
   5 files changed, 11 insertions(+)
   create mode 100644 a/b/.joshinfo
   create mode 100644 a/b/file3
   create mode 100644 c/.joshinfo
   create mode 100644 c/file1
   create mode 100644 c/file2
  $ git reset --hard
  HEAD is now at * sync (glob)

  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file3
  `-- c
      |-- file1
      `-- file2
  
  3 directories, 3 files
  $ git log --graph --pretty=%s
  * sync
  * initial

  $ cat c/.joshinfo
  commit: * (glob)
  src: libs/master
  tree: * (glob)
  view: :/sub1

  $ cat a/b/.joshinfo
  commit: * (glob)
  src: libs/foo
  tree: * (glob)
  view: :/sub2

$ git show libs/master | grep $(cat c/.joshinfo | grep commit | sed 's/commit: //')
commit * (glob)
$ git show libs/foo | grep $(cat a/b/.joshinfo | grep commit | sed 's/commit: //')
commit * (glob)

