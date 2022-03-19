  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents4 > sub1/file4
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir ws
  $ echo contents5 > ws/file5
  $ git add ws
  $ git commit -m "add file5" 1> /dev/null
  $ cat > ws/workspace.josh <<EOF
  > ::ws/file5
  > :/sub1::file1
  > ::sub2/subsub/
  > a = :/sub1
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/sub1
  [1] :/subsub
  [1] ::file1
  [1] :[
      ::file1
      :prefix=a
  ]
  [1] :prefix=a
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :/sub2
  [2] ::ws/file5
  [3] :[
      :/sub1:[
          ::file1
          :prefix=a
      ]
      ::sub2/subsub/
      ::ws/file5
  ]
  [3] :workspace=ws

  $ git log --graph --pretty=%s refs/josh/master
  *   add ws
  |\  
  | * add file5
  | * add file2
  | * add file1
  * add file5

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  |-- a
  |   `-- file4
  |-- file1
  |-- file5
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  3 directories, 5 files
