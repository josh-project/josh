  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add sub2/file2" 1> /dev/null

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add sub1/file1" 1> /dev/null
  $ git branch branch1

  $ echo contents1 > sub1/file2
  $ echo contents2 > sub2/fileX
  $ git add .
  $ git commit -m "add sub1/file2 sub2/fileX" 1> /dev/null

  $ git log --graph --oneline --decorate
  * 0d0a43d (HEAD -> master) add sub1/file2 sub2/fileX
  * 8a87424 (branch1) add sub1/file1
  * 4d74643 add sub2/file2

  $ git checkout branch1
  Switched to branch 'branch1'
  $ echo contents3 > sub2/fileY
  $ echo contents6 > sub1/fileY
  $ git add .
  $ git commit -m "add sub_/fileY" 1> /dev/null
  $ git log --graph --oneline --decorate
  * 96cc30d (HEAD -> branch1) add sub_/fileY
  * 8a87424 add sub1/file1
  * 4d74643 add sub2/file2

  $ josh-filter -s ::sub1/ branch1 --update refs/heads/hidden_branch1
  [2] :prefix=sub1
  [3] :/sub1
  $ git checkout hidden_branch1
  Switched to branch 'hidden_branch1'
  $ git log --graph --oneline --decorate
  * 81a8353 (HEAD -> hidden_branch1) add sub_/fileY
  * 7671c2a add sub1/file1
  $ echo contents3 > sub1/file3
  $ git add .
  $ git commit -m "add sub1/file3" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'

  $ josh-filter -s ::sub1/ master --update refs/heads/hidden_master
  [3] :prefix=sub1
  [4] :/sub1
  $ git checkout hidden_master
  Switched to branch 'hidden_master'
  $ git log --graph --oneline --decorate
  * 586737e (HEAD -> hidden_master) add sub1/file2 sub2/fileX
  * 7671c2a add sub1/file1
  $ echo contents4 > sub1/file4
  $ git add sub1/file4
  $ git commit -m "add sub1/file4" 1> /dev/null

  $ git log hidden_master --graph --oneline --decorate
  * 6f816ed (HEAD -> hidden_master) add sub1/file4
  * 586737e add sub1/file2 sub2/fileX
  * 7671c2a add sub1/file1
  $ git log hidden_branch1 --graph --oneline --decorate
  * 24a7e40 (hidden_branch1) add sub1/file3
  * 81a8353 add sub_/fileY
  * 7671c2a add sub1/file1

  $ git merge -q hidden_branch1 --no-ff
  $ git log --graph --oneline
  *   2fcb6a4 Merge branch 'hidden_branch1' into hidden_master
  |\  
  | * 24a7e40 add sub1/file3
  | * 81a8353 add sub_/fileY
  * | 6f816ed add sub1/file4
  * | 586737e add sub1/file2 sub2/fileX
  |/  
  * 7671c2a add sub1/file1

  $ josh-filter -s ::sub1/ --reverse master --update refs/heads/hidden_master
  [3] :prefix=sub1
  [4] :/sub1

  $ git checkout master
  Switched to branch 'master'
  $ git status
  On branch master
  nothing to commit, working tree clean

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   |-- file2
  |   |-- file3
  |   |-- file4
  |   `-- fileY
  `-- sub2
      |-- file2
      `-- fileX
  
  3 directories, 7 files

  $ git log --graph --oneline --decorate master
  *   8cbae19 (HEAD -> master) Merge branch 'hidden_branch1' into hidden_master
  |\  
  | * 7d3be00 add sub1/file3
  | * 5e05ab3 add sub_/fileY
  * | 4428b57 add sub1/file4
  * | 0d0a43d add sub1/file2 sub2/fileX
  |/  
  * 8a87424 add sub1/file1
  * 4d74643 add sub2/file2
  $ git log --graph --oneline --decorate branch1
  * 96cc30d (branch1) add sub_/fileY
  * 8a87424 add sub1/file1
  * 4d74643 add sub2/file2

  $ git diff 4428b57..8cbae19 --stat
   sub1/file3 | 1 +
   sub1/fileY | 1 +
   2 files changed, 2 insertions(+)
  $ git diff 7d3be00..8cbae19 --stat
   sub1/file2 | 1 +
   sub1/file4 | 1 +
   sub2/fileX | 1 +
   3 files changed, 3 insertions(+)
  $ git diff 5e05ab3..7d3be00 --stat
   sub1/file3 | 1 +
   1 file changed, 1 insertion(+)
  $ git diff 5e05ab3 --stat
   sub1/file2 | 1 +
   sub1/file3 | 1 +
   sub1/file4 | 1 +
   sub2/fileX | 1 +
   4 files changed, 4 insertions(+)
  $ git diff 4428b57..7d3be00 --stat
   sub1/file2 | 1 -
   sub1/file3 | 1 +
   sub1/file4 | 1 -
   sub1/fileY | 1 +
   sub2/fileX | 1 -
   5 files changed, 2 insertions(+), 3 deletions(-)
