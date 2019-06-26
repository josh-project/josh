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

  $ cat > syncinfo <<EOF
  > [remotes/libs/master:sync/master]
  > c = :/sub1
  > [remotes/libs/foo:sync/foo]
  > a/b = :/sub2
  > EOF

  $ git add syncinfo
  $ git commit -m "initial" &> /dev/null

  $ josh-sync --file syncinfo
  $ git log --graph --pretty=%s sync/master
  * add file2
  * add file1
  $ git log --graph --pretty=%s sync/foo
  * add file3

  $ josh-sync --squash --file syncinfo
  $ git log --graph --pretty=%s sync/master
  * add file2
  $ git log --graph --pretty=%s sync/foo
  * add file3

  $ git read-tree HEAD sync/master sync/foo
  $ git commit -m "sync"
  [master *] sync (glob)
   5 files changed, 7 insertions(+)
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
  |-- c
  |   |-- file1
  |   `-- file2
  `-- syncinfo
  
  3 directories, 4 files
  $ git log --graph --pretty=%s
  * sync
  * initial

  $ cat c/.joshinfo
  commit: * (glob)
  target: remotes/libs/master

  $ cat a/b/.joshinfo
  commit: * (glob)
  target: remotes/libs/foo

  $ git show libs/master | grep $(cat c/.joshinfo | grep commit | sed 's/commit: //')
  commit * (glob)
  $ git show libs/foo | grep $(cat a/b/.joshinfo | grep commit | sed 's/commit: //')
  commit * (glob)

