  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init 1> /dev/null

  $ mkdir a
  $ echo contents1 > a/file_a1
  $ echo contents1 > a/file_a2
  $ git add a

  $ mkdir b
  $ echo contents1 > b/file_b1
  $ git add b

  $ mkdir -p c/d
  $ echo contents1 > c/d/file_cd
  $ git add c
  $ git commit -m "add dirs" 1> /dev/null

  $ echo contents2 > c/d/file_cd2
  $ git add c
  $ git commit -m "add file_cd2" 1> /dev/null

  $ mkdir -p c/d/e
  $ echo contents2 > c/d/e/file_cd3
  $ git add c
  $ git commit -m "add file_cd3" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file_cd3
  * add file_cd2
  * add dirs

  $ josh-filter master:refs/josh/filtered :dirs

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   `-- JOSH_ORIG_PATH
  |-- b
  |   `-- JOSH_ORIG_PATH
  `-- c
      |-- JOSH_ORIG_PATH
      `-- d
          |-- JOSH_ORIG_PATH
          `-- e
              `-- JOSH_ORIG_PATH
  
  5 directories, 5 files

  $ cat $(find . -name JOSH_ORIG_PATH) | sort
  a/
  b/
  c/
  c/d/
  c/d/e/

  $ josh-filter master:refs/josh/filtered :dirs:/c

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- JOSH_ORIG_PATH
  `-- d
      |-- JOSH_ORIG_PATH
      `-- e
          `-- JOSH_ORIG_PATH
  
  2 directories, 3 files

  $ cat $(find . -name JOSH_ORIG_PATH) | sort
  c/
  c/d/
  c/d/e/

  $ josh-filter master:refs/josh/filtered :dirs:/a

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- JOSH_ORIG_PATH
  
  0 directories, 1 file

  $ cat $(find . -name JOSH_ORIG_PATH) | sort
  a/

  $ josh-filter master:refs/josh/filtered :dirs:hide=c:prefix=x

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- x
      |-- a
      |   `-- JOSH_ORIG_PATH
      `-- b
          `-- JOSH_ORIG_PATH
  
  3 directories, 2 files

  $ cat $(find . -name JOSH_ORIG_PATH) | sort
  a/
  b/
