  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}
  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > :/sub1:glob=file1
  > sub2/subsub
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter master --update refs/josh/master :workspace=ws

  $ git log --graph --pretty=%s refs/josh/master
  * add ws
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  |-- file1
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  2 directories, 3 files
