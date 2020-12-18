Empty root commits from unrelated parts of the tree should not be included

  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs 1>/dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ echo contents2 > sub1/file3
  $ git add sub1
  $ git commit -m "add file3" 1> /dev/null

  $ josh-filter c=:/sub1 master --update refs/josh/filter/master

  $ git log refs/josh/filter/master --graph --pretty=%s
  * add file3
  * add file2
  * add file1


  $ git ls-tree --name-only -r refs/josh/filter/master 
  c/file1
  c/file2
  c/file3

  $ git checkout --orphan other
  Switched to a new branch 'other'
  $ git reset --hard
  $ git status
  On branch other
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)
  $ git commit --allow-empty -m "root" 1> /dev/null
  $ git ls-tree -r HEAD

  $ echo contents2 > some_file
  $ git add some_file
  $ git commit -m "add some_file" 1>/dev/null

  $ echo contents2 > some_other_file
  $ git add some_other_file
  $ git commit -m "add some_other_file" 1>/dev/null

  $ git checkout master
  Switched to branch 'master'
  $ git merge other --no-ff --allow-unrelated
  Merge made by the 'recursive' strategy.
   some_file       | 1 +
   some_other_file | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 some_file
   create mode 100644 some_other_file

  $ tree
  .
  |-- some_file
  |-- some_other_file
  `-- sub1
      |-- file1
      |-- file2
      `-- file3
  
  1 directory, 5 files

  $ git log master --graph --pretty=%s
  *   Merge branch 'other'
  |\  
  | * add some_other_file
  | * add some_file
  | * root
  * add file3
  * add file2
  * add file1


  $ josh-filter c=:/sub1 master

  $ git log JOSH_HEAD --graph --pretty=%s
  * add file3
  * add file2
  * add file1

  $ git ls-tree --name-only -r JOSH_HEAD 
  c/file1
  c/file2
  c/file3

  $ josh-filter c=:hide=sub1 master

  $ git log JOSH_HEAD --graph --pretty=%s
  * add some_other_file
  * add some_file
  * root

  $ git ls-tree --name-only -r JOSH_HEAD 
  c/some_file
  c/some_other_file

  $ josh-filter :prefix=x JOSH_HEAD

  $ git ls-tree --name-only -r JOSH_HEAD
  x/c/some_file
  x/c/some_other_file

  $ git ls-tree --name-only -r JOSH_HEAD~1
  x/c/some_file

  $ git ls-tree --name-only -r JOSH_HEAD~2

Make sure that even with prefix applied we get a proper empty tree here
  $ git show --format=raw JOSH_HEAD~2 | grep tree
  tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904
