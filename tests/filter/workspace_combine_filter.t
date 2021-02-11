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

  $ mkdir -p sub3
  $ echo contents1 > sub3/sub_file
  $ git add .
  $ git commit -m "add sub_file" 1> /dev/null

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > x = :[::sub2/subsub/,::sub1/]
  > EOF
  $ mkdir ws2
  $ cat > ws2/workspace.josh <<EOF
  > :[
  >   a = :[::sub2/subsub/,::sub3/]
  >   :/sub1:prefix=blub
  > ]:prefix=xyz
  > EOF
  $ git add .
  $ git commit -m "add ws" 1> /dev/null

  $ tree
  .
  |-- sub1
  |   `-- file1
  |-- sub2
  |   `-- subsub
  |       `-- file2
  |-- sub3
  |   `-- sub_file
  |-- ws
  |   `-- workspace.josh
  `-- ws2
      `-- workspace.josh
  
  6 directories, 5 files

  $ josh-filter -s :workspace=ws
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
  [2] :prefix=x
  [2] :workspace=ws

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add ws
  * add file2
  * add file1

  $ git checkout FILTERED_HEAD 2> /dev/null
  $ tree
  .
  |-- workspace.josh
  `-- x
      |-- sub1
      |   `-- file1
      `-- sub2
          `-- subsub
              `-- file2
  
  4 directories, 3 files

  $ git checkout master 2> /dev/null
  $ josh-filter -s :workspace=ws2
  [1] :/sub1
  [1] :/subsub
  [1] :prefix=blub
  [1] :prefix=sub1
  [1] :prefix=sub2
  [1] :prefix=sub3
  [1] :prefix=subsub
  [2] :/sub2
  [2] :/sub3
  [2] :[
      ::sub1/
      ::sub2/subsub/
  ]
  [2] :prefix=a
  [2] :prefix=x
  [2] :workspace=ws
  [2] :workspace=ws2
  [3] :[
      ::sub2/subsub/
      ::sub3/
  ]
  [3] :[
      a = :[
          ::sub2/subsub/
          ::sub3/
      ]
      blub = :/sub1
  ]
  [3] :prefix=xyz

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add ws
  * add sub_file
  * add file2
  * add file1

  $ git checkout FILTERED_HEAD 2> /dev/null
  $ tree
  .
  |-- workspace.josh
  `-- xyz
      |-- a
      |   |-- sub2
      |   |   `-- subsub
      |   |       `-- file2
      |   `-- sub3
      |       `-- sub_file
      `-- blub
          `-- file1
  
  6 directories, 4 files
