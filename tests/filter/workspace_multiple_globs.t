  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents1 > sub1/file2
  $ chmod +x sub1/file2
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git ls-tree -r HEAD
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tsub1/file1 (esc)
  100755 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tsub1/file2 (esc)

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a = ::**/file2
  > b = ::**/file1
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] ::**/file1
  [1] :prefix=b
  [2] ::**/file2
  [2] :[
      a = ::**/file2
      b = ::**/file1
  ]
  [2] :prefix=a
  [2] :workspace=ws

  $ git log --graph --pretty=%s refs/josh/master
  * add ws
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ git ls-tree HEAD
  040000 tree 911c4952e2ea6662b24ba38f173bd25a0ea30f25\ta (esc)
  040000 tree c82fc150c43f13cc56c0e9caeba01b58ec612022\tb (esc)
  100644 blob ad7c87a358d35432edbe44a052b7b7731ca3103f\tworkspace.josh (esc)
  $ tree
  .
  |-- a
  |   |-- sub1
  |   |   `-- file2
  |   `-- sub2
  |       `-- subsub
  |           `-- file2
  |-- b
  |   `-- sub1
  |       `-- file1
  `-- workspace.josh
  
  6 directories, 4 files
