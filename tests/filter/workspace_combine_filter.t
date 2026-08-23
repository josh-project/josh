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

  $ git-tree-pretty .
  .
  ├── sub1/
  │   └── file1
  │       ┆  contents1
  ├── sub2/
  │   └── subsub/
  │       └── file2
  │           ┆  contents1
  ├── sub3/
  │   └── sub_file
  │       ┆  contents1
  ├── ws/
  │   └── workspace.josh
  │       ┆  x = :[::sub2/subsub/,::sub1/]
  └── ws2/
      └── workspace.josh
          ┆  :[
          ┆    a = :[::sub2/subsub/,::sub3/]
          ┆    :/sub1:prefix=blub
          ┆  ]:prefix=xyz

  $ josh-filter -s :workspace=ws
  0fe13f59da8ba3f49ca610f2354254747502fa40
  [2] :[
      ::sub1/
      ::sub2/subsub/
  ]
  [2] :prefix=x
  [2] :workspace=ws
  [4] reachable_roots
  [4] sequence_number

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add ws
  * add file2
  * add file1

  $ git-tree-pretty FILTERED_HEAD
  .
  ├── workspace.josh
  │   ┆  x = :[::sub2/subsub/,::sub1/]
  └── x/
      ├── sub1/
      │   └── file1
      │       ┆  contents1
      └── sub2/
          └── subsub/
              └── file2
                  ┆  contents1

  $ josh-filter -s :workspace=ws2
  8a0812406d4c375a5f173abc839b00ad3b7fc30d
  [2] :[
      ::sub1/
      ::sub2/subsub/
  ]
  [2] :prefix=x
  [2] :workspace=ws
  [2] :workspace=ws2
  [3] :[
      a = :[
          ::sub2/subsub/
          ::sub3/
      ]
      blub = :/sub1
  ]
  [3] :prefix=xyz
  [7] reachable_roots
  [7] sequence_number

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add ws
  * add sub_file
  * add file2
  * add file1

  $ git-tree-pretty FILTERED_HEAD
  .
  ├── workspace.josh
  │   ┆  :[
  │   ┆    a = :[::sub2/subsub/,::sub3/]
  │   ┆    :/sub1:prefix=blub
  │   ┆  ]:prefix=xyz
  └── xyz/
      ├── a/
      │   ├── sub2/
      │   │   └── subsub/
      │   │       └── file2
      │   │           ┆  contents1
      │   └── sub3/
      │       └── sub_file
      │           ┆  contents1
      └── blub/
          └── file1
              ┆  contents1
