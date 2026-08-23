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
  > a = :/sub1:exclude[::file1]
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/heads/filtered
  d64cfa7819992ce9bfe0ee335b802928fb5df337
  [2] :[
      a = :/sub1:exclude[::file1]
      ::sub2/subsub/
  ]
  [2] :workspace=ws
  [3] reachable_roots
  [3] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  * add ws
  * add file2
  * add file1

  $ git checkout filtered
  Switched to branch 'filtered'
  $ git-tree-pretty .
  .
  ├── a/
  │   └── file4
  │       ┆  contents4
  ├── sub2/
  │   └── subsub/
  │       └── file2
  │           ┆  contents1
  └── workspace.josh
      ┆  ::sub2/subsub/
      ┆  a = :/sub1:exclude[::file1]

  $ echo ws_content > fileX
  $ echo ws_content > file1
  $ git add .
  $ git commit -m "add 1X" 1> /dev/null

  $ josh-filter -s :workspace=ws --reverse master --update refs/heads/filtered
  fdfbfdcdc052aeb50f5033ec3d7f0cee3340d253
  [2] :[
      a = :/sub1:exclude[::file1]
      ::sub2/subsub/
  ]
  [2] :workspace=ws
  [3] reachable_roots
  [3] sequence_number
  $ josh-filter -s :workspace=ws --reverse --check-roundtrip master --update refs/heads/filtered
  Roundtrip failed
  [2] :[
      a = :/sub1:exclude[::file1]
      ::sub2/subsub/
  ]
  [3] :workspace=ws
  [9] reachable_roots
  [9] sequence_number
  $ git checkout master
  Switched to branch 'master'

  $ git-tree-pretty .
  .
  ├── sub1/
  │   ├── file1
  │   │   ┆  contents1
  │   └── file4
  │       ┆  contents4
  ├── sub2/
  │   └── subsub/
  │       └── file2
  │           ┆  contents1
  └── ws/
      ├── file1
      │   ┆  ws_content
      ├── fileX
      │   ┆  ws_content
      └── workspace.josh
          ┆  a = :/sub1:exclude[::file1]
          ┆  ::sub2/subsub/
