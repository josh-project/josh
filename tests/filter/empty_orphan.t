Empty root commits from unrelated parts of the tree should not be included

  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs &>/dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" &> /dev/null

  $ josh-filter master:refs/josh/filter/master c=:/sub1

  $ git log refs/josh/filter/master --graph --pretty=%s
  * add file2
  * add file1


  $ git ls-tree --name-only -r refs/josh/filter/master 
  c/file1
  c/file2

  $ git checkout --orphan other
  Switched to a new branch 'other'
  $ git commit --allow-empty -m "root" &> /dev/null

  $ echo contents2 > some_file
  $ git add some_file
  $ git commit -m "add some_file" &>/dev/null

  $ git checkout master
  Switched to branch 'master'
  $ git merge other --no-ff --allow-unrelated
  Merge made by the 'recursive' strategy.
   some_file | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 some_file

  $ git log master --graph --pretty=%s
  *   Merge branch 'other'
  |\  
  | * add some_file
  | * root
  * add file2
  * add file1

  $ josh-filter master:refs/josh/filter/master c=:/sub1

  $ git log refs/josh/filter/master --graph --pretty=%s
  * add file2
  * add file1

  $ git ls-tree --name-only -r refs/josh/filter/master 
  c/file1
  c/file2
