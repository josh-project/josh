  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}
  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter master:refs/josh/master :workspace=ws

  $ git log --graph --pretty=%s refs/josh/master
  * add ws
  * add file2
  * add file1

  $ mkdir -p ws/c
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c/ = :/sub1
  > EOF
  $ git add ws
  $ git commit -m "add trailing slash" 1> /dev/null

  $ josh-filter master:refs/josh/master :workspace=ws
  ERROR: JoshError("converted Error { code: -1, klass: 14, message: \"failed to insert entry: invalid name for a tree entry - c/\" }")
  [1]

  $ git log --graph --pretty=%s refs/josh/master
  * add ws
  * add file2
  * add file1

  $ git checkout -q refs/josh/master 1> /dev/null
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- file1
  |-- workspace.josh
  `-- ws
      `-- c
  
  5 directories, 3 files

  $ git checkout -q HEAD~1
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- file1
  `-- ws
      `-- c
  
  5 directories, 2 files

