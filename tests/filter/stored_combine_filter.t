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

  $ mkdir -p sub3
  $ echo contents1 > sub3/sub_file
  $ git add .
  $ git commit -m "add sub_file" 1> /dev/null

  $ mkdir st
  $ cat > st/config.josh <<EOF
  > x = :[::sub2/subsub/,::sub1/]
  > EOF
  $ mkdir st2
  $ cat > st2/config.josh <<EOF
  > :[
  >   a = :[::sub2/subsub/,::sub3/]
  >   :/sub1:prefix=blub
  > ]:prefix=xyz
  > EOF
  $ git add .
  $ git commit -m "add st" 1> /dev/null

  $ tree
  .
  |-- st
  |   `-- config.josh
  |-- st2
  |   `-- config.josh
  |-- sub1
  |   `-- file1
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- sub3
      `-- sub_file
  
  7 directories, 5 files

  $ josh-filter -s :+st/config
  [2] :+st/config
  [2] :[
      ::sub1/
      ::sub2/subsub/
  ]
  [2] :prefix=x
  [4] sequence_number

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add st
  * add file2
  * add file1

  $ git checkout FILTERED_HEAD 2> /dev/null
  $ tree
  .
  |-- st
  |   `-- config.josh
  `-- x
      |-- sub1
      |   `-- file1
      `-- sub2
          `-- subsub
              `-- file2
  
  6 directories, 3 files

  $ git checkout master 2> /dev/null
  $ josh-filter -s :+st2/config
  [2] :+st/config
  [2] :+st2/config
  [2] :[
      ::sub1/
      ::sub2/subsub/
  ]
  [2] :prefix=x
  [3] :[
      a = :[
          ::sub2/subsub/
          ::sub3/
      ]
      blub = :/sub1
  ]
  [3] :prefix=xyz
  [7] sequence_number

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add st
  * add sub_file
  * add file2
  * add file1

  $ git checkout FILTERED_HEAD 2> /dev/null
  $ tree
  .
  |-- st2
  |   `-- config.josh
  `-- xyz
      |-- a
      |   |-- sub2
      |   |   `-- subsub
      |   |       `-- file2
      |   `-- sub3
      |       `-- sub_file
      `-- blub
          `-- file1
  
  8 directories, 4 files

