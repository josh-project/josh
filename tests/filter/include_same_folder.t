  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir ws
  $ echo contents1 > ws/file1
  $ git add ws/file1
  $ git commit -m "add file1"
  [master (root-commit) 26a7c70] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 ws/file1

  $ mkdir sub1
  $ echo contents2 > sub1/file2
  $ git add sub1/file2
  $ git commit -m "add file2"
  [master 7b0b2f8] add file2
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2

  $ mkdir sub2
  $ echo contents3 > sub2/file3
  $ git add sub2/file3
  $ git commit -m "add file3"
  [master 184c02d] add file3
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file3

  $ cat > ws/deps.josh << EOF
  > ::sub1/
  > EOF
  $ git add ws/deps.josh
  $ git commit -m "add deps.josh"
  [master 27e392b] add deps.josh
   1 file changed, 1 insertion(+)
   create mode 100644 ws/deps.josh

  $ cat > ws/workspace.josh << EOF
  > :include=ws/deps.josh
  > EOF
  $ git add ws/workspace.josh
  $ git commit -m "add workspace.josh"
  [master 24bc83e] add workspace.josh
   1 file changed, 1 insertion(+)
   create mode 100644 ws/workspace.josh

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :prefix=sub1
  [2] :/sub1
  [2] :include=ws/deps.josh
  [3] :workspace=ws
  $ git checkout refs/josh/master 2>/dev/null
  $ tree
  .
  |-- deps.josh
  |-- file1
  |-- sub1
  |   `-- file2
  `-- workspace.josh
  
  1 directory, 4 files
  $ git log --oneline --graph
  *   ab5e400 add workspace.josh
  |\  
  | * b70ec25 add deps.josh
  | * fccda12 add file2
  * a11175e add deps.josh
  * 0b4cf6c add file1
