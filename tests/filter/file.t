  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null


  $ mkdir sub2
  $ echo contents1 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null


  $ cat > file.josh <<EOF
  > c = :/sub1
  > a/b = :/sub2
  > EOF

  $ git add file.josh
  $ git commit -m "initial" 1> /dev/null

  $ josh-filter -s --file file.josh
  [3] :[
      c = :/sub1
      a/b = :/sub2
  ]
  $ git log --graph --pretty=%s FILTERED_HEAD
  * add file3
  * add file2
  * add file1

  $ josh-filter -s --single --file file.josh
  [4] :[
      c = :/sub1
      a/b = :/sub2
  ]
  $ git log --graph --pretty=%s FILTERED_HEAD
  * initial

  $ tree .git/refs/
  .git/refs/
  |-- heads
  |   `-- master
  `-- tags
  
  3 directories, 1 file

