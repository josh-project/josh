  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub2
  $ mkdir subx
  $ echo contentsroot > rootfile
  $ echo contents1 > sub2/file1
  $ echo contentsx > subx/filex
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub2/file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null

  $ tree
  .
  |-- rootfile
  |-- sub2
  |   |-- file1
  |   `-- file2
  `-- subx
      `-- filex
  
  3 directories, 4 files

  $ josh-filter -s :exclude[::sub2/] master --update refs/heads/hidden
  [1] :exclude[::sub2/]
  [2] sequence_number
  $ git checkout hidden 1> /dev/null
  Switched to branch 'hidden'
  $ tree
  .
  |-- rootfile
  `-- subx
      `-- filex
  
  2 directories, 2 files
  $ git log --graph --pretty=%s
  * add file1

  $ echo new_root > rootfile
  $ echo new_x >> subx/filex
  $ git add .
  $ git commit -m "edit files" 1> /dev/null

  $ josh-filter -s :exclude[::sub2/] --reverse master --update refs/heads/hidden
  [1] :exclude[::sub2/]
  [2] sequence_number

  $ git checkout master
  Switched to branch 'master'

  $ git log -1
  commit 55031991a5c2f493f2d62201828d8f20844ab219
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      edit files

  $ git diff HEAD~1
  diff --git a/rootfile b/rootfile
  index b58e324..a49be3e 100644
  --- a/rootfile
  +++ b/rootfile
  @@ -1 +1 @@
  -contentsroot
  +new_root
  diff --git a/subx/filex b/subx/filex
  index d1cc012..4e41ce2 100644
  --- a/subx/filex
  +++ b/subx/filex
  @@ -1 +1,2 @@
   contentsx
  +new_x

  $ tree
  .
  |-- rootfile
  |-- sub2
  |   |-- file1
  |   `-- file2
  `-- subx
      `-- filex
  
  3 directories, 4 files

