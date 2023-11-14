  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
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

  $ josh-filter -s :exclude[::sub2/] master --update refs/heads/hidden
  [1] :exclude[::sub2/]
  $ git checkout hidden 1> /dev/null
  Switched to branch 'hidden'
  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file
  $ git log --graph --pretty=%s
  * add file1

  $ echo contents3 > sub1/file3
  $ git add sub1/file3
  $ git commit -m "add sub1/file3" 1> /dev/null

  $ josh-filter -s :exclude[::sub2/] --reverse master --update refs/heads/hidden
  [1] :exclude[::sub2/]

  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file3
  `-- sub2
      `-- file2
  
  3 directories, 3 files


  $ cat sub1/file3
  contents3

  $ git log --graph --pretty=%s
  * add sub1/file3
  * add file2
  * add file1

  $ git checkout hidden 1> /dev/null
  Switched to branch 'hidden'

  $ mkdir sub2
  $ echo contents4 > sub2/file4
  $ git add sub2/file4
  $ git commit -m "add sub2/file4" 1> /dev/null
  $ git commit -m "empty commit" --allow-empty 1> /dev/null

  $ josh-filter -s :exclude[::sub2/] --reverse master --update refs/heads/hidden
  [2] :exclude[::sub2/]
  $ git log --graph --pretty=%s refs/heads/master
  * empty commit
  * add sub1/file3
  * add file2
  * add file1
