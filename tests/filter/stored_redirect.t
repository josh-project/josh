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

  $ mkdir st
  $ cat > st/config.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > EOF
  $ echo "foobar" > st/extra_file
  $ git add st
  $ git commit -m "add st" 1> /dev/null

  $ mkdir sub3
  $ echo contents3 > sub3/file4
  $ git add sub3
  $ git commit -m "add file4" 1> /dev/null

  $ cat > st/config.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > b = :/sub3
  > EOF
  $ git add st
  $ git commit -m "edit st" 1> /dev/null

  $ mkdir st_new
  $ echo "foobar" > st_new/extra_file_new
  $ cat > st_new/config.josh <<EOF
  > :+st/config
  > EOF
  $ git add st_new
  $ git commit -m "add st_new" 1> /dev/null

  $ josh-filter -s :+st/config master --update refs/heads/filtered
  6c3969ebc7f3a9286e3b94fea28646aaaa9021b1
  [1] :prefix=b
  [2] :/sub3
  [2] :subtract[
          :[
              st = :/st::config.josh
              a = :/sub1
              ::sub2/subsub/
          ]
          :/st::config.josh
      ]
  [3] :+st/config
  [7] reachable_roots
  [7] sequence_number
  $ josh-filter -s :+st_new/config master --update refs/heads/filtered_new
  5f2b78a024c1abd991085bea441509b6c252eb93
  [1] :prefix=b
  [2] :+st_new/config
  [2] :/sub3
  [2] :subtract[
          :[
              st = :/st::config.josh
              a = :/sub1
              ::sub2/subsub/
          ]
          :/st::config.josh
      ]
  [3] :+st/config
  [5] :subtract[
          :[
              st = :/st::config.josh
              st_new = :/st_new::config.josh
              a = :/sub1
              ::sub2/subsub/
              b = :/sub3
          ]
          :/st_new::config.josh
      ]
  [7] reachable_roots
  [7] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  *   edit st
  |\  
  | * add file4
  * add st
  * add file2
  * add file1
  $ git log --graph --pretty=%s refs/heads/filtered_new
  * add st_new
  * edit st
  * add file4
  * add st
  * add file2
  * add file1

  $ git-tree-pretty refs/heads/filtered
  .
  ├── a/
  │   ├── file1
  │   │   ┆  contents1
  │   └── file4
  │       ┆  contents4
  ├── b/
  │   └── file4
  │       ┆  contents3
  ├── st/
  │   └── config.josh
  │       ┆  ::sub2/subsub/
  │       ┆  a = :/sub1
  │       ┆  b = :/sub3
  └── sub2/
      └── subsub/
          └── file2
              ┆  contents1
  $ git-tree-pretty refs/heads/filtered_new
  .
  ├── a/
  │   ├── file1
  │   │   ┆  contents1
  │   └── file4
  │       ┆  contents4
  ├── b/
  │   └── file4
  │       ┆  contents3
  ├── st/
  │   └── config.josh
  │       ┆  ::sub2/subsub/
  │       ┆  a = :/sub1
  │       ┆  b = :/sub3
  ├── st_new/
  │   └── config.josh
  │       ┆  :+st/config
  └── sub2/
      └── subsub/
          └── file2
              ┆  contents1


  $ cat > st/config.josh <<EOF
  > :+st_new/config
  > EOF
  $ git add st
  $ git commit -m "add st recursion" 1> /dev/null

  $ josh-filter -s :+st/config master --update refs/heads/filtered
  267d7721cca8af54f01f44756b10ede7b22ef238
  [1] :prefix=b
  [1] :prefix=st_new
  [2] :+st_new/config
  [2] :/sub3
  [2] :subtract[
          :/st_new::config.josh
          :[
              a = :/sub1
              ::sub2/subsub/
              b = :/sub3
          ]
      ]
  [2] :subtract[
          :[
              st = :/st::config.josh
              a = :/sub1
              ::sub2/subsub/
          ]
          :/st::config.josh
      ]
  [4] :+st/config
  [5] :subtract[
          :[
              st = :/st::config.josh
              st_new = :/st_new::config.josh
              a = :/sub1
              ::sub2/subsub/
              b = :/sub3
          ]
          :/st_new::config.josh
      ]
  [9] reachable_roots
  [9] sequence_number

