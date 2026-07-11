  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir -p st/c
  $ cat > st/config.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ echo st_content > st/c/file1
  $ git add st
  $ git commit -m "add st" 1> /dev/null

  $ git log --graph --pretty=%s
  * add st
  * add file2
  * add file1
  $ tree
  .
  |-- st
  |   |-- c
  |   |   `-- file1
  |   `-- config.josh
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- file2
  
  5 directories, 4 files

  $ cat sub1/file1
  contents1
  $ cat st/c/file1
  st_content

  $ josh-filter :+st/config master --update refs/heads/st
  99b3384cb31ab9d642bd2e5e4050d57f97fe2862
  $ git checkout st 1> /dev/null
  Switched to branch 'st'
  $ git log --graph --pretty=%s
  * add st
  * add file2
  * add file1
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- file1
  `-- st
      `-- config.josh
  
  5 directories, 3 files

  $ cat c/file1
  contents1

  $ echo modified_content > c/file1
  $ echo contents3 > st_created_file
  $ git add .
  $ git commit -m "modify and add files" 1> /dev/null

  $ josh-filter :+st/config master --update refs/heads/st --reverse
  3cbdd590e0f190a08fc64b34386a3dcc698e177f

  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- st
  |   |-- c
  |   |   `-- file1
  |   `-- config.josh
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- file2
  
  5 directories, 4 files

  $ cat sub1/file1
  modified_content
  $ cat st/st_created_file
  cat: st/st_created_file: No such file or directory
  [1]

  $ git log --graph --pretty=%s
  * modify and add files
  * add st
  * add file2
  * add file1

