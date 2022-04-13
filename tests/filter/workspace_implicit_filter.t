  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
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
  > ::sub2/subsub/
  > ::sub1/
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/sub1
  [1] :/subsub
  [1] :prefix=sub1
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :/sub2
  [2] :[
      ::sub1/
      ::sub2/subsub/
  ]
  [2] :workspace=ws

  $ git log --graph --pretty=%s refs/josh/master
  * add ws
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  |-- sub1
  |   `-- file1
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  3 directories, 3 files
