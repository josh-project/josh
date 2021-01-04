  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init 1> /dev/null

  $ mkdir a
  $ echo "cws = :/c" > a/workspace.josh
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

  $ josh-filter -s :DIRS master --update refs/josh/filtered
  [3 -> 2] :DIRS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- JOSH_ORIG_PATH_a
  |   `-- workspace.josh
  |-- b
  |   `-- JOSH_ORIG_PATH_b
  `-- c
      |-- JOSH_ORIG_PATH_c
      `-- d
          |-- JOSH_ORIG_PATH_c%2Fd
          `-- e
              `-- JOSH_ORIG_PATH_c%2Fd%2Fe
  
  5 directories, 6 files

  $ josh-filter -s :DIRS:/c master --update refs/josh/filtered
  [2 -> 2] :/c
  [3 -> 2] :DIRS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- JOSH_ORIG_PATH_c
  `-- d
      |-- JOSH_ORIG_PATH_c%2Fd
      `-- e
          `-- JOSH_ORIG_PATH_c%2Fd%2Fe
  
  2 directories, 3 files


  $ josh-filter -s :DIRS:/a master --update refs/josh/filtered
  [2 -> 1] :/a
  [2 -> 2] :/c
  [3 -> 2] :DIRS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- JOSH_ORIG_PATH_a
  `-- workspace.josh
  
  0 directories, 2 files


  $ josh-filter -s :DIRS:hide=c:prefix=x master --update refs/josh/filtered
  [1 -> 1] :prefix=x
  [2 -> 1] :/a
  [2 -> 1] :hide=c
  [2 -> 2] :/c
  [3 -> 2] :DIRS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- x
      |-- a
      |   |-- JOSH_ORIG_PATH_a
      |   `-- workspace.josh
      `-- b
          `-- JOSH_ORIG_PATH_b
  
  3 directories, 3 files



  $ git checkout master 2> /dev/null
  $ git rm -r c/d
  rm 'c/d/e/file_cd3'
  rm 'c/d/file_cd'
  rm 'c/d/file_cd2'
  $ git commit -m "rm" 1> /dev/null

  $ echo contents2 > a/newfile
  $ git add a
  $ git commit -m "add newfile" 1> /dev/null

  $ josh-filter -s :DIRS master --update refs/josh/filtered
  [1 -> 1] :prefix=x
  [2 -> 1] :/a
  [2 -> 1] :hide=c
  [2 -> 2] :/c
  [5 -> 3] :DIRS

  $ git log --graph --pretty=%s master
  * add newfile
  * rm
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git log --graph --pretty=%s refs/josh/filtered
  * rm
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- JOSH_ORIG_PATH_a
  |   `-- workspace.josh
  `-- b
      `-- JOSH_ORIG_PATH_b
  
  2 directories, 3 files


  $ josh-filter -s :DIRS:FOLD master --update refs/josh/filtered
  [1 -> 1] :prefix=x
  [2 -> 1] :/a
  [2 -> 1] :hide=c
  [2 -> 2] :/c
  [3 -> 2] :FOLD
  [5 -> 3] :DIRS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- JOSH_ORIG_PATH_a
  |   `-- workspace.josh
  |-- b
  |   `-- JOSH_ORIG_PATH_b
  `-- c
      |-- JOSH_ORIG_PATH_c
      `-- d
          |-- JOSH_ORIG_PATH_c%2Fd
          `-- e
              `-- JOSH_ORIG_PATH_c%2Fd%2Fe
  
  5 directories, 6 files


  $ josh-filter -s :DIRS:/c:FOLD master --update refs/josh/filtered
  [1 -> 1] :prefix=x
  [2 -> 1] :/a
  [2 -> 1] :hide=c
  [3 -> 3] :/c
  [5 -> 3] :DIRS
  [6 -> 4] :FOLD

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- JOSH_ORIG_PATH_c
  `-- d
      |-- JOSH_ORIG_PATH_c%2Fd
      `-- e
          `-- JOSH_ORIG_PATH_c%2Fd%2Fe
  
  2 directories, 3 files


  $ josh-filter -s :DIRS:workspace=a:FOLD master --update refs/josh/filtered
  [1 -> 1] :prefix=x
  [2 -> 1] :/a
  [2 -> 1] :hide=c
  [3 -> 3] :/c
  [3 -> 3] :workspace=a
  [5 -> 3] :DIRS
  [9 -> 6] :FOLD

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- JOSH_ORIG_PATH_a
  |-- cws
  |   |-- JOSH_ORIG_PATH_c
  |   `-- d
  |       |-- JOSH_ORIG_PATH_c%2Fd
  |       `-- e
  |           `-- JOSH_ORIG_PATH_c%2Fd%2Fe
  `-- workspace.josh
  
  3 directories, 5 files

