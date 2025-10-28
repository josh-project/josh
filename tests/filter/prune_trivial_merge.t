  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file1

  $ git checkout -b branch1
  Switched to a new branch 'branch1'
  $ echo contents2 > file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'

  $ echo contents3 > file1
  $ git add .
  $ git commit -m "mod file1" 1> /dev/null

  $ git commit --allow-empty -m "empty commit" 1> /dev/null

  $ git merge -q  branch1 --no-ff
  $ git log --graph --pretty=%s
  *   Merge branch 'branch1'
  |\  
  | * add file2
  * | empty commit
  * | mod file1
  |/  
  * add file1

  $ josh-filter -s ::file1
  [4] ::file1
  [5] sequence_number
  $ git log --graph --pretty=%s FILTERED_HEAD
  *   Merge branch 'branch1'
  |\  
  * | empty commit
  * | mod file1
  |/  
  * add file1
  $ josh-filter -s ::file1:prune=trivial-merge
  [3] :prune=trivial-merge
  [4] ::file1
  [6] sequence_number

  $ git log --graph --pretty=%s FILTERED_HEAD
  * empty commit
  * mod file1
  * add file1




