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
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8
  [1] :exclude[::sub2/]
  [2] sequence_number
  $ git checkout hidden 1> /dev/null
  Switched to branch 'hidden'
  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file
  $ git log --graph --pretty=%s
  * add file1

  $ echo contents3 >> sub1/file1
  $ git add sub1
  $ git commit -m "edit file1" 1> /dev/null

  $ josh-filter -s :exclude[::sub2/] --reverse master --update refs/heads/hidden
  04a66ac914f2040990c1a47c7dc152fe02b1c337
  [1] :exclude[::sub2/]
  [2] sequence_number

  $ git checkout master
  Switched to branch 'master'

  $ git log -1
  commit 04a66ac914f2040990c1a47c7dc152fe02b1c337
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      edit file1

  $ git diff HEAD~1
  diff --git a/sub1/file1 b/sub1/file1
  index a024003..96a94d5 100644
  --- a/sub1/file1
  +++ b/sub1/file1
  @@ -1 +1,2 @@
   contents1
  +contents3

  $ tree
  .
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- file2
  
  3 directories, 2 files

