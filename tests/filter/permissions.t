  $ export TESTTMP=${PWD}

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

  $ echo contents3 >> c/d/e/file_cd3
  $ git add c
  $ git commit -m "edit file_cd3" 1> /dev/null

  $ git log --graph --pretty=%s
  * edit file_cd3
  * add file_cd3
  * add file_cd2
  * add dirs

  $ josh-filter -s :PATHS master --update refs/josh/filtered
  [3] :PATHS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 6 files

  $ josh-filter -s :PATHS:/c master --update refs/josh/filtered
  [3] :/c
  [3] :PATHS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- d
      |-- e
      |   `-- file_cd3
      |-- file_cd
      `-- file_cd2
  
  2 directories, 3 files


  $ josh-filter -s :PATHS:/a master --update refs/josh/filtered
  [1] :/a
  [3] :/c
  [3] :PATHS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- file_a2
  `-- workspace.josh
  
  0 directories, 2 files


  $ josh-filter -s :PATHS:exclude[:/c]:prefix=x master --update refs/josh/filtered
  [1] :/a
  [1] :exclude[:/c]
  [1] :prefix=x
  [3] :/c
  [3] :PATHS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- x
      |-- a
      |   |-- file_a2
      |   `-- workspace.josh
      `-- b
          `-- file_b1
  
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

  $ josh-filter -s :PATHS master --update refs/josh/filtered
  [1] :/a
  [1] :exclude[:/c]
  [1] :prefix=x
  [3] :/c
  [5] :PATHS

  $ git log --graph --pretty=%s master
  * add newfile
  * rm
  * edit file_cd3
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git log --graph --pretty=%s refs/josh/filtered
  * add newfile
  * rm
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   |-- newfile
  |   `-- workspace.josh
  `-- b
      `-- file_b1
  
  2 directories, 4 files


  $ josh-filter -s :PATHS:FOLD master --update refs/josh/filtered
  [1] :/a
  [1] :exclude[:/c]
  [1] :prefix=x
  [3] :/c
  [4] :FOLD
  [5] :PATHS

  $ git log --graph --pretty=%s refs/josh/filtered
  * add newfile
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   |-- newfile
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 7 files


  $ josh-filter -s :PATHS:/c:FOLD master --update refs/josh/filtered
  [1] :/a
  [1] :exclude[:/c]
  [1] :prefix=x
  [4] :/c
  [5] :PATHS
  [7] :FOLD

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- d
      |-- e
      |   `-- file_cd3
      |-- file_cd
      `-- file_cd2
  
  2 directories, 3 files


  $ josh-filter -s :PATHS:workspace=a:FOLD master --update refs/josh/filtered
  [1] :/a
  [1] :exclude[:/c]
  [1] :prefix=x
  [2] :workspace=a
  [4] :/c
  [5] :PATHS
  [9] :FOLD

  $ git log --graph --pretty=%s refs/josh/filtered
  * add newfile
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- file_a2
  |-- newfile
  `-- workspace.josh
  
  0 directories, 3 files

