  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs &>/dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null

  $ git checkout -b foo
  Switched to a new branch 'foo'

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" &> /dev/null

  $ cd ${TESTTMP}
  $ git init apps &>/dev/null
  $ cd apps

  $ cat > syncinfo <<EOF
  > [sync/master: ${TESTTMP}/libs @ refs/heads/master]
  > c = :/sub1
  > [sync/foo: ${TESTTMP}/libs @ refs/heads/foo]
  > a/b = :/sub2
  > EOF

  $ git add syncinfo
  $ git commit -m "initial" &> /dev/null

  $ josh-fetch --file syncinfo
  warning: no common commits
  From */libs (glob)
   * branch            master     -> FETCH_HEAD
  warning: no common commits
  From */libs (glob)
   * branch            foo        -> FETCH_HEAD

  $ git read-tree HEAD sync/master sync/foo
  $ git commit -m "sync"
  [master *] sync (glob)
   4 files changed, 4 insertions(+)
   create mode 100644 a/b/file2
   create mode 100644 a/b/joshinfo
   create mode 100644 c/file1
   create mode 100644 c/joshinfo
  $ git reset --hard
  HEAD is now at * sync (glob)
  $ tree
  .
  |-- a
  |   `-- b
  |       |-- file2
  |       `-- joshinfo
  |-- c
  |   |-- file1
  |   `-- joshinfo
  `-- syncinfo
  
  3 directories, 5 files
  $ git log --graph --pretty=%s
  * sync
  * initial

  $ cat a/b/joshinfo
  e8fee4624fc8543bf1e8c46cd57917d1ac72d68f
  $ cat c/joshinfo
  540c562ab79645772fa38ee78fd5f4d76059bde2
