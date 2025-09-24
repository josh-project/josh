  $ export TESTTMP=${PWD}


  $ cd ${TESTTMP}
  $ mkdir remote1 remote2
  $ cd remote1
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
  $ echo contents3 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ cd ${TESTTMP}/remote2
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo different1 > sub1/file1
  $ git add sub1
  $ git commit -m "add different file1" 1> /dev/null

  $ mkdir sub2
  $ echo different2 > sub2/file3
  $ git add sub2
  $ git commit -m "add different file3" 1> /dev/null

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

  $ josh clone ${TESTTMP}/remote1/libs:/sub1
  Successfully added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin
  Successfully cloned repository to: libs

  $ cd libs

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ josh remote add remote2 ${TESTTMP}/remote2/libs:/sub2
  Successfully added remote 'remote2' with filter ':/sub2:prune=trivial-merge'

  $ josh pull --remote remote2
  Successfully fetched from remote: remote2
  Successfully pulled from remote: remote2

  $ tree
  .
  `-- file3
  
  1 directory, 1 file

  $ josh pull
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin

  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files
