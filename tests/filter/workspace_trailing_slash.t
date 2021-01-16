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

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/sub1
  [1] :/sub2
  [1] :prefix=a
  [1] :prefix=b
  [1] :prefix=c
  [1] :workspace=ws
  [2] :[
      c = :/sub1
      a/b = :/sub2
  ]

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

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/sub1
  [1] :/sub2
  [1] :prefix=a
  [1] :prefix=b
  [1] :prefix=c
  [2] :[
      c = :/sub1
      a/b = :/sub2
  ]
  [2] :workspace=ws

  $ git log --graph --pretty=%s refs/josh/master
  * add trailing slash
  * add ws
  * add file2
  * add file1

  $ git checkout -q refs/josh/master 1> /dev/null
  $ tree
  .
  |-- workspace.josh
  `-- ws
      `-- c
  
  2 directories, 1 file

  $ git checkout -q HEAD~1
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

