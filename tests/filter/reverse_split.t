  $ export TESTTMP=${PWD}
  $ export FILTER=":[a=::*.a,rest=:/]"

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ echo contents1 > file1.a
  $ git add .
  $ git commit -m "add file1.a" 1> /dev/null

  $ echo contents1 > file2.b
  $ git add .
  $ git commit -m "add file2.b" 1> /dev/null

  $ josh-filter -s $FILTER master --update refs/heads/filtered
  [1] ::*.a
  [1] :prefix=a
  [2] :[
      a = ::*.a
      :prefix=rest
  ]
  [2] :prefix=rest
  $ git checkout filtered 1> /dev/null
  Switched to branch 'filtered'
  $ tree
  .
  |-- a
  |   `-- file1.a
  `-- rest
      `-- file2.b
  
  3 directories, 2 files
  $ git log --graph --pretty=%s
  * add file2.b
  * add file1.a

  $ echo contents3 >> a/file3.a
  $ echo contents3 >> rest/file4.b
  $ git add .
  $ git commit -m "add files" 1> /dev/null

  $ josh-filter -s $FILTER --reverse master --update refs/heads/filtered
  [1] ::*.a
  [1] :prefix=a
  [2] :[
      a = ::*.a
      :prefix=rest
  ]
  [2] :prefix=rest

  $ git checkout master
  Switched to branch 'master'

  $ git log -1
  commit 4031be37b86723bab26952dcd055a4d7294aa827
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add files

  $ git diff HEAD~1
  diff --git a/file3.a b/file3.a
  new file mode 100644
  index 0000000..1cb5d64
  --- /dev/null
  +++ b/file3.a
  @@ -0,0 +1 @@
  +contents3
  diff --git a/file4.b b/file4.b
  new file mode 100644
  index 0000000..1cb5d64
  --- /dev/null
  +++ b/file4.b
  @@ -0,0 +1 @@
  +contents3

  $ tree
  .
  |-- file1.a
  |-- file2.b
  |-- file3.a
  `-- file4.b
  
  1 directory, 4 files

