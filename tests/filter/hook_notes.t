  $ git init -q 1> /dev/null

  $ mkdir a
  $ mkdir b
  $ mkdir c
  $ echo a > a/f
  $ echo b > b/g
  $ echo c > c/h
  $ git add .
  $ git commit -m init 1> /dev/null
  $ git notes add -m '::a' -f

  $ echo a > a/f2
  $ echo b > b/g2
  $ git add .
  $ git commit -m "add f2" 1> /dev/null
  $ git notes add -m '::a' -f

  $ echo a > c/f3
  $ git add .
  $ git commit -m "add f3" 1> /dev/null
  $ git notes add -m ':[::a,::b]' -f


  $ josh-filter -s :hook=commits HEAD --update refs/josh/filtered
  [2] ::b
  [3] :hook="commits"

  $ git log --graph --pretty=%s refs/josh/filtered
  *   add f3
  |\  
  | * add f2
  | * init
  * add f2
  * init
