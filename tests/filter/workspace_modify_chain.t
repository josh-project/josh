  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never
  $ export FILTER=":workspace=ws:/sub2"

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
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter -s $FILTER master --update refs/heads/filtered
  [1] :/subsub
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :workspace=ws
  [3] :/sub2

  $ git log --graph --pretty=%s refs/heads/filtered
  * add file2

  $ git checkout filtered
  Switched to branch 'filtered'
  $ tree
  .
  `-- subsub
      `-- file2
  
  2 directories, 1 file

  $ echo ws_content > subsub/fileX
  $ echo ws_content > subsub/file1
  $ git add .
  $ git commit -m "add 1X" 1> /dev/null

  $ josh-filter -s $FILTER --reverse master --update refs/heads/filtered
  [1] :/subsub
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :workspace=ws
  [3] :/sub2
  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file4
  |-- sub2
  |   `-- subsub
  |       |-- file1
  |       |-- file2
  |       `-- fileX
  `-- ws
      `-- workspace.josh
  
  5 directories, 6 files

  $ cat ws/file1
  *: No such file or directory (glob)
  [1]
  $ cat sub1/file1
  contents1
