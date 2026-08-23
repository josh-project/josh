  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ mkdir sub2
  $ echo contents2 > sub2/file2
  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub1 sub2 sub3
  $ git commit -m "add files" 1> /dev/null

Test basic scope filter syntax :<X>[Y]
  $ FILTER_HASH=$(josh-filter -i ':<:/sub1>[:/file1]')
  $ josh-filter -p ${FILTER_HASH}
  sub1 = :/sub1/file1
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── chain/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  sub1
      ├── 1/
      │   └── subdir/
      │       └── 0
      │           ╵  file1
      └── 2/
          └── prefix/
              └── 0
                  ╵  sub1

Test scope filter with multiple filters in compose
  $ FILTER_HASH=$(josh-filter -i ':<:/sub1>[:/file1,:/sub2/file2]')
  $ josh-filter -p ${FILTER_HASH}
  sub1 = :/sub1:[
      :/file1
      :/sub2/file2
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── chain/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  sub1
      ├── 1/
      │   └── compose/
      │       ├── 0/
      │       │   └── subdir/
      │       │       └── 0
      │       │           ╵  file1
      │       └── 1/
      │           └── chain/
      │               ├── 0/
      │               │   └── subdir/
      │               │       └── 0
      │               │           ╵  sub2
      │               └── 1/
      │                   └── subdir/
      │                       └── 0
      │                           ╵  file2
      └── 2/
          └── prefix/
              └── 0
                  ╵  sub1

Test scope filter with prefix filter
  $ FILTER_HASH=$(josh-filter -i ':<:prefix=sub1>[:prefix=file1]')
  $ josh-filter -p ${FILTER_HASH}
  :empty
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── empty

Test scope filter with subdir and exclude
  $ FILTER_HASH=$(josh-filter -i ':<:/sub1>[:exclude[::file1]]')
  $ josh-filter -p ${FILTER_HASH}
  sub1 = :/sub1:exclude[::file1]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── chain/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  sub1
      ├── 1/
      │   └── exclude/
      │       └── 0/
      │           └── file/
      │               ├── 0
      │               │   ╵  file1
      │               └── 1
      │                   ╵  file1
      └── 2/
          └── prefix/
              └── 0
                  ╵  sub1

Test scope filter verifies it expands to chain(X, chain(Y, invert(X)))
  $ FILTER_HASH=$(josh-filter -i ':<:/sub1>[:/file1]')
  $ josh-filter --print-filter ${FILTER_HASH}
  error: unexpected argument found
  [2]

Test scope filter with nested filters
  $ FILTER_HASH=$(josh-filter -i ':<:/sub1>[:[:/file1,:/sub2/file2]]')
  $ josh-filter -p ${FILTER_HASH}
  sub1 = :/sub1:[
      :/file1
      :/sub2/file2
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── chain/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  sub1
      ├── 1/
      │   └── compose/
      │       ├── 0/
      │       │   └── subdir/
      │       │       └── 0
      │       │           ╵  file1
      │       └── 1/
      │           └── chain/
      │               ├── 0/
      │               │   └── subdir/
      │               │       └── 0
      │               │           ╵  sub2
      │               └── 1/
      │                   └── subdir/
      │                       └── 0
      │                           ╵  file2
      └── 2/
          └── prefix/
              └── 0
                  ╵  sub1

