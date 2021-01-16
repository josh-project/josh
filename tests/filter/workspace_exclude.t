  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init real_repo 1> /dev/null
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
  [1] :/sub1
  [1] :/sub2
  [1] :/subsub
  [1] ::file1
  [1] :SUBTRACT[:nop~::file1]
  [1] :prefix=a
  [1] :prefix=sub2
  [1] :prefix=subsub
  [1] :workspace=ws
  [2] :[:/sub1:SUBTRACT[:nop~::file1]:prefix=a,:/sub2:/subsub:prefix=subsub:prefix=sub2]

  $ git log --graph --pretty=%s refs/heads/filtered
  * add ws
  * add file2
  * add file1

  $ git checkout filtered
  Switched to branch 'filtered'
  $ tree
  .
  |-- a
  |   `-- file4
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  3 directories, 3 files

  $ echo ws_content > fileX
  $ echo ws_content > file1
  $ git add .
  $ git commit -m "add 1X" 1> /dev/null

  $ josh-filter -s :workspace=ws --reverse master --update refs/heads/filtered
  [1] :/sub1
  [1] :/sub2
  [1] :/subsub
  [1] ::file1
  [1] :SUBTRACT[:nop~::file1]
  [1] :prefix=a
  [1] :prefix=sub2
  [1] :prefix=subsub
  [1] :workspace=ws
  [2] :[:/sub1:SUBTRACT[:nop~::file1]:prefix=a,:/sub2:/subsub:prefix=subsub:prefix=sub2]
  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file4
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- ws
      |-- file1
      |-- fileX
      `-- workspace.josh
  
  4 directories, 6 files

  $ cat ws/file1
  ws_content
  $ cat sub1/file1
  contents1
