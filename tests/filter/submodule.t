  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ cd ${TESTTMP}
  $ git init app 1> /dev/null
  $ cd app
  $ git commit -m "init" --allow-empty 1> /dev/null
  $ git submodule add ../libs 2> /dev/null
  $ git submodule status
   * libs (heads/master) (glob)

  $ git commit -m "add libs" 1> /dev/null

  $ git log --graph --pretty=%s
  * add libs
  * init

  $ josh-filter master:refs/josh/filter/master :/libs
  $ git ls-tree --name-only -r refs/josh/filter/master 
  fatal: Not a valid object name refs/josh/filter/master
  [128]
  $ josh-filter master:refs/josh/filter/master c=:/libs
  $ git ls-tree --name-only -r refs/josh/filter/master 

$ git log refs/josh/filter/master --graph --pretty=%s
* add file2
* add file1

$ git ls-tree --name-only -r refs/josh/filter/master 
c/file1
c/file2

$ git rm -r sub1
rm 'sub1/file1'
rm 'sub1/file2'
$ git commit -m "rm sub1" 1> /dev/null

$ josh-filter master:refs/josh/filter/master c=:/sub1

$ git log refs/josh/filter/master --graph --pretty=%s
* rm sub1
* add file2
* add file1

$ git ls-tree --name-only -r refs/josh/filter/master 
