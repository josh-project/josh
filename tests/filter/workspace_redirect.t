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
  $ cat > ws/workspace.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > EOF
  $ echo "foobar" > ws/extra_file
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ mkdir sub3
  $ echo contents3 > sub3/file4
  $ git add sub3
  $ git commit -m "add file4" 1> /dev/null

  $ cat > ws/workspace.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > b = :/sub3
  > EOF
  $ git add ws
  $ git commit -m "edit ws" 1> /dev/null

  $ mkdir ws_new
  $ echo "foobar" > ws_new/extra_file_new
  $ cat > ws_new/workspace.josh <<EOF
  > :workspace=ws
  > EOF
  $ git add ws_new
  $ git commit -m "add ws_new" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/heads/filtered
  b9624622ee21a49c687f9fe717803c10aeb7829d
  [1] :prefix=b
  [2] :/sub3
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [3] :workspace=ws
  [7] reachable_roots
  [7] sequence_number
  $ josh-filter -s :workspace=ws_new master --update refs/heads/filtered_new
  b9624622ee21a49c687f9fe717803c10aeb7829d
  [1] :prefix=b
  [2] :/sub3
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [2] :workspace=ws_new
  [3] :workspace=ws
  [5] :exclude[::ws_new]
  [7] reachable_roots
  [7] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  *   edit ws
  |\  
  | * add file4
  * add ws
  * add file2
  * add file1
  $ git log --graph --pretty=%s refs/heads/filtered_new
  *   edit ws
  |\  
  | * add file4
  * add ws
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
  ├── extra_file
  │   ┆  foobar
  ├── sub2/
  │   └── subsub/
  │       └── file2
  │           ┆  contents1
  └── workspace.josh
      ┆  ::sub2/subsub/
      ┆  a = :/sub1
      ┆  b = :/sub3
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
  ├── extra_file
  │   ┆  foobar
  ├── sub2/
  │   └── subsub/
  │       └── file2
  │           ┆  contents1
  └── workspace.josh
      ┆  ::sub2/subsub/
      ┆  a = :/sub1
      ┆  b = :/sub3


  $ cat > ws/workspace.josh <<EOF
  > :workspace=ws_new
  > EOF
  $ git add ws
  $ git commit -m "add ws recursion" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/heads/filtered
  0000000000000000000000000000000000000000
  [1] :prefix=b
  [2] :/sub3
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [3] :workspace=ws_new
  [4] :exclude[::ws]
  [4] :workspace=ws
  [6] :exclude[::ws_new]
  [10] reachable_roots
  [10] sequence_number
