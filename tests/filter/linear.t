  $ git init -q 1> /dev/null

  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file1

  $ git checkout -b branch2
  Switched to a new branch 'branch2'

  $ echo contents2 > file1
  $ git add .
  $ git commit -m "mod file1" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'

  $ echo contents3 > file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null

  $ git merge -q branch2 --no-ff

  $ git log --graph --pretty=%s
  *   Merge branch 'branch2'
  |\  
  | * mod file1
  * | add file2
  |/  
  * add file1

  $ josh-filter -s :linear refs/heads/master --update refs/heads/filtered
  d24e7038b232dc1bd6d803d211e92039229375b4
  [4] :linear
  [4] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  * Merge branch 'branch2'
  * add file2
  * add file1

  $ git ls-tree --name-only -r refs/heads/filtered
  file1
  file2

  $ git checkout filtered
  Switched to branch 'filtered'

  $ echo contents4 > file2
  $ git add .
  $ git commit -m "mod file2" 1> /dev/null

  $ git log --graph --pretty=%s refs/heads/filtered
  * mod file2
  * Merge branch 'branch2'
  * add file2
  * add file1

  $ josh-filter -s :linear refs/heads/master --update refs/heads/filtered --reverse
  65fb0dcfe9fd24ab4d7027ff1359bd44847bd21a
  d24e7038b232dc1bd6d803d211e92039229375b4
  [4] :linear
  [4] sequence_number

  $ git log --graph --pretty=%s refs/heads/master
  * mod file2
  *   Merge branch 'branch2'
  |\  
  | * mod file1
  * | add file2
  |/  
  * add file1

  $ git log --graph --pretty=%s refs/heads/filtered
  * mod file2
  * Merge branch 'branch2'
  * add file2
  * add file1