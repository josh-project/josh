  $ export TESTTMP=${PWD}
  $ export "PATH=${TESTDIR}/../../target/debug/:${PATH}"

  $ cd ${TESTTMP}
  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir -p ws/c
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ echo ws_content > ws/c/file1
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ git log --graph --pretty=%s
  * add ws
  * add file2
  * add file1
  $ tree
  .
  |-- sub1
  |   `-- file1
  |-- sub2
  |   `-- file2
  `-- ws
      |-- c
      |   `-- file1
      `-- workspace.josh
  
  4 directories, 4 files

  $ cat sub1/file1
  contents1
  $ cat ws/c/file1
  ws_content

  $ josh-filter :workspace=ws master --update refs/heads/ws
  $ git checkout ws 1> /dev/null
  Switched to branch 'ws'
  $ git log --graph --pretty=%s
  * add ws
  * add file2
  * add file1
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- file1
  `-- workspace.josh
  
  3 directories, 3 files

  $ cat c/file1
  ws_content

  $ echo contents3 > ws_created_file
  $ git add ws_created_file
  $ git commit -m "add ws_created_file" 1> /dev/null

  $ josh-filter :workspace=ws master --update refs/heads/ws --reverse

  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- sub1
  |   `-- file1
  |-- sub2
  |   `-- file2
  `-- ws
      |-- a
      |   `-- b
      |       `-- file2
      |-- c
      |   `-- file1
      |-- workspace.josh
      `-- ws_created_file
  
  6 directories, 6 files

  $ cat sub1/file1
  contents1

  $ git log --graph --pretty=%s
  * add ws_created_file
  * add ws
  * add file2
  * add file1
