  $ export TESTTMP=${PWD}

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

  $ mkdir sub3
  $ echo contents1 > sub3/file1
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ josh-filter -s :exclude[::sub2/] master --update refs/heads/hidden
  [1] :prefix=sub2
  [2] :/sub2
  [2] :exclude[::sub2/]
  $ git checkout -q hidden 1> /dev/null
  $ tree
  .
  |-- sub1
  |   `-- file1
  `-- sub3
      `-- file1
  
  2 directories, 2 files
  $ git log --graph --pretty=%s
  * add file3
  * add file1

  $ echo contents3 > sub1/file3
  $ git add sub1/file3
  $ git commit -m "add sub1/file3" 1> /dev/null

  $ josh-filter -s :exclude[::sub1/,::sub2/] master --update refs/josh/filtered
  [1] :/sub1
  [1] :prefix=sub1
  [1] :prefix=sub2
  [2] :/sub2
  [2] :[
      ::sub1/
      ::sub2/
  ]
  [2] :exclude[
      ::sub1/
      ::sub2/
  ]
  [2] :exclude[::sub2/]

  $ git checkout -q refs/josh/filtered
  $ tree
  .
  `-- sub3
      `-- file1
  
  1 directory, 1 file

  $ josh-filter -s :exclude[sub1=:/sub3] master --update refs/josh/filtered
  [1] :/sub1
  [1] :prefix=sub2
  [2] :/sub2
  [2] :/sub3
  [2] :[
      ::sub1/
      ::sub2/
  ]
  [2] :exclude[
      ::sub1/
      ::sub2/
  ]
  [2] :exclude[::sub2/]
  [2] :prefix=sub1
  [3] :exclude[:/sub3:prefix=sub1]

  $ git checkout -q refs/josh/filtered
  $ tree
  .
  |-- sub2
  |   `-- file2
  `-- sub3
      `-- file1
  
  2 directories, 2 files
