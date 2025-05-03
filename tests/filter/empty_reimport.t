  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q testrepo 1> /dev/null
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

  $ git merge -q other_branch
  $ git log --graph --pretty=%s
  *   Merge branch 'other_branch'
  |\  
  | * change on other 2
  | * change on other
  * | unrelated change on this
  |/  
  * commit1


  $ josh-filter -s c=:/pre master --update refs/josh/filter/master
  [2] :prefix=c
  [4] :/pre

  $ git log josh/filter/master --graph --pretty=%s
  * change on other 2
  * change on other

  $ git checkout other_branch
  Switched to branch 'other_branch'

  $ echo content > pre/blah-file1
  $ git add pre
  $ git commit -m "more change on other" 1> /dev/null

  $ echo content > pre/blah-file2
  $ git add pre
  $ git commit -m "more change on other 2" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'

  $ echo content > testfile7
  $ git add .
  $ git commit -m "more unrelated change on this" 1> /dev/null
  $ git merge -q other_branch

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
  [5] :prefix=c
  [7] :/pre

  $ git log josh/filter/master --graph --pretty=%s
  *   Merge branch 'other_branch'
  |\  
  | * more change on other 2
  | * more change on other
  |/  
  * change on other 2
  * change on other
