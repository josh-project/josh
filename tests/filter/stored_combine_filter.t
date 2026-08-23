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

  $ git-tree-pretty .
  .
  ├── st/
  │   └── config.josh
  │       ┆  x = :[::sub2/subsub/,::sub1/]
  ├── st2/
  │   └── config.josh
  │       ┆  :[
  │       ┆    a = :[::sub2/subsub/,::sub3/]
  │       ┆    :/sub1:prefix=blub
  │       ┆  ]:prefix=xyz
  ├── sub1/
  │   └── file1
  │       ┆  contents1
  ├── sub2/
  │   └── subsub/
  │       └── file2
  │           ┆  contents1
  └── sub3/
      └── sub_file
          ┆  contents1

  $ josh-filter -s :+st/config
  9527d0249f419b172c6ca02390fde00f81e9c078
  [2] :+st/config
  [2] :subtract[
          :[
              st = :/st::config.josh
              x = :[
                  ::sub1/
                  ::sub2/subsub/
              ]
          ]
          :/st::config.josh
      ]
  [4] reachable_roots
  [4] sequence_number

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add st
  * add file2
  * add file1

  $ git-tree-pretty FILTERED_HEAD
  .
  ├── st/
  │   └── config.josh
  │       ┆  x = :[::sub2/subsub/,::sub1/]
  └── x/
      ├── sub1/
      │   └── file1
      │       ┆  contents1
      └── sub2/
          └── subsub/
              └── file2
                  ┆  contents1

  $ josh-filter -s :+st2/config
  f9e1862628d454b0cc4e98305983335d9c615113
  [2] :+st/config
  [2] :+st2/config
  [2] :subtract[
          :[
              st = :/st::config.josh
              x = :[
                  ::sub1/
                  ::sub2/subsub/
              ]
          ]
          :/st::config.josh
      ]
  [3] :subtract[
          :[
              st2 = :/st2::config.josh
              xyz = :[
                  a = :[
                      ::sub2/subsub/
                      ::sub3/
                  ]
                  blub = :/sub1
              ]
          ]
          :/st2::config.josh
      ]
  [4] reachable_roots
  [4] sequence_number

  $ git log --graph --pretty=%s FILTERED_HEAD
  * add st
  * add sub_file
  * add file2
  * add file1

  $ git-tree-pretty FILTERED_HEAD
  .
  ├── st2/
  │   └── config.josh
  │       ┆  :[
  │       ┆    a = :[::sub2/subsub/,::sub3/]
  │       ┆    :/sub1:prefix=blub
  │       ┆  ]:prefix=xyz
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

