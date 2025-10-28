  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
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

Note that we are trying to relocate the workspace.josh file, which is not possible
so the workspace.josh will still appear in the root of the workspace
  $ cat > ws/workspace.josh <<EOF
  > :/sub1::file1
  > ::sub2/subsub/
  > a = :/sub1
  > b = ::ws/workspace.josh
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [2] :[
      :/sub1:[
          ::file1
          :prefix=a
      ]
      ::sub2/subsub/
      b = ::ws/workspace.josh
  ]
  [2] :workspace=ws
  [3] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * add ws
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  |-- a
  |   `-- file4
  |-- file1
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  4 directories, 4 files
