  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init testrepo 1> /dev/null
  $ cd testrepo

  $ echo contents1 > testfile
  $ git add testfile
  $ git commit -m "commit1" 1> /dev/null

  $ git checkout -b other_branch
  Switched to a new branch 'other_branch'

  $ mkdir pre
  $ echo content > pre/testfile2
  $ git add pre
  $ git commit -m "change on other" 1> /dev/null

  $ echo content > pre/testfile4
  $ git add pre
  $ git commit -m "change on other 2" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'
  $ echo content > testfile3
  $ git add .
  $ git commit -m "unrelated change on this" 1> /dev/null

  $ git merge other_branch
  Merge made by the 'recursive' strategy.
   pre/testfile2 | 1 +
   pre/testfile4 | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 pre/testfile2
   create mode 100644 pre/testfile4
  $ git log --graph --pretty=%s
  *   Merge branch 'other_branch'
  |\  
  | * change on other 2
  | * change on other
  * | unrelated change on this
  |/  
  * commit1


  $ josh-filter -s c=:/pre master --update refs/josh/filter/master
  [2] :/pre
  [2] :prefix=c

  $ git log josh/filter/master --graph --pretty=%s
  * change on other 2
  * change on other

  $ git checkout other_branch
  Switched to branch 'other_branch'

  $ echo content > pre/blafile1
  $ git add pre
  $ git commit -m "more change on other" 1> /dev/null

  $ echo content > pre/blefile2
  $ git add pre
  $ git commit -m "more change on other 2" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'

  $ echo content > testfile7
  $ git add .
  $ git commit -m "more unrelated change on this" 1> /dev/null
  $ git merge other_branch
  Merge made by the 'recursive' strategy.
   pre/blafile1 | 1 +
   pre/blefile2 | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 pre/blafile1
   create mode 100644 pre/blefile2

  $ git log --graph --pretty=%s
  *   Merge branch 'other_branch'
  |\  
  | * more change on other 2
  | * more change on other
  * | more unrelated change on this
  * | Merge branch 'other_branch'
  |\| 
  | * change on other 2
  | * change on other
  * | unrelated change on this
  |/  
  * commit1


  $ josh-filter -s c=:/pre master --update refs/josh/filter/master
  [5] :/pre
  [5] :prefix=c

  $ git log josh/filter/master --graph --pretty=%s
  *   Merge branch 'other_branch'
  |\  
  | * more change on other 2
  | * more change on other
  |/  
  * change on other 2
  * change on other
