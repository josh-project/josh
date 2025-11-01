  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null


  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents111 > file1
  $ git add .
  $ git commit -m "mod file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * mod file1
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

  $ echo contents11 > file1
  $ git add .
  $ git commit -m "mod file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * mod file1
  *   Merge branch 'branch1'
  |\  
  | * add file2
  * | empty commit
  * | mod file1
  |/  
  * mod file1
  * add file1

  $ josh-filter -s ::file1
  [4] ::file1
  $ git log --graph --pretty=%s FILTERED_HEAD
  * mod file1
  * mod file1
  * mod file1
  * add file1

  $ cat > file.josh <<EOF
  > ::file1:rev(
  >   0000000000000000000000000000000000000000:prune=trivial-merge
  >   2aae7efbe08b6fd7a5e88df974d4c1817968282f:/
  > )
  > EOF

  $ josh-filter -s --file file.josh
  [4] :prune=trivial-merge
  [5] ::file1
  [5] :rev(0000000000000000000000000000000000000000:prune=trivial-merge,2aae7efbe08b6fd7a5e88df974d4c1817968282f:/)

  $ git log --graph --pretty=%s FILTERED_HEAD
  * mod file1
  *   Merge branch 'branch1'
  |\  
  * | empty commit
  * | mod file1
  |/  
  * mod file1
  * add file1
  $ josh-filter -s ::file1:prune=trivial-merge
  [3] :prune=trivial-merge
  [4] ::file1

  $ git log --graph --pretty=%s FILTERED_HEAD
  * empty commit
  * mod file1
  *   Merge branch 'branch1'
  |\  
  * | mod file1
  |/  
  * mod file1
  * add file1




