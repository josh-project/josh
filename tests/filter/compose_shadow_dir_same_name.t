  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir xx
  $ echo contents1 > xx/file2
  $ git add xx
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir -p sub/xx
  $ echo contents1 > sub/xx/file3
  $ echo contents1 > sub/xx/file4
  $ git add sub
  $ git commit -m "add file3" 1> /dev/null

  $ git-tree-pretty HEAD
  .
  ├── sub/
  │   └── xx/
  │       ├── file3
  │       │   ┆  contents1
  │       └── file4
  │           ┆  contents1
  ├── sub1/
  │   └── file1
  │       ┆  contents1
  └── xx/
      └── file2
          ┆  contents1


  $ josh-filter ":[:/sub1,:/xx]"
  11f4a718fc1c49fdda1b3ebce22efec68683edaf
  $ git-tree-pretty FILTERED_HEAD
  .
  ├── file1
  │   ┆  contents1
  └── file2
      ┆  contents1

  $ josh-filter ":[:/xx,:/sub1]"
  11f4a718fc1c49fdda1b3ebce22efec68683edaf
  $ git-tree-pretty FILTERED_HEAD
  .
  ├── file1
  │   ┆  contents1
  └── file2
      ┆  contents1

  $ josh-filter -s ":[:/sub/xx::file3,:/sub1,:/xx,:/sub/xx]"
  f9da6dcfc582a60447a9870b596eb9f28a7e03ec
  [2] :[
      :/sub1
      :/xx
  ]
  [2] :[
      :/xx
      :/sub1
  ]
  [3] :[
      :/sub/xx::file3
      :/sub1
      :/xx
      :/sub/xx
  ]
  [3] reachable_roots
  [3] sequence_number
  $ git-tree-pretty FILTERED_HEAD
  .
  ├── file1
  │   ┆  contents1
  ├── file2
  │   ┆  contents1
  ├── file3
  │   ┆  contents1
  └── file4
      ┆  contents1
