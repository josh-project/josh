  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ git checkout -b foo
  Switched to a new branch 'foo'

  $ mkdir sub2
  $ echo contents1 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ cd ${TESTTMP}
  $ git init apps 1> /dev/null
  $ cd apps

  $ git remote add libs ${TESTTMP}/libs
  $ git fetch --all
  Fetching libs
  From * (glob)
   * [new branch]      foo        -> libs/foo
   * [new branch]      master     -> libs/master


  $ cat > syncinfo <<EOF
  > [libs(master)]
  > c = :/sub1
  > [libs(foo)]
  > a/b = :/sub2
  > EOF

  $ git add syncinfo
  $ git commit -m "initial" 1> /dev/null

  $ git ls-tree -r HEAD
  100644 blob 078fc2cc27af0d3d32e98f297a7e315019474562\tsyncinfo (esc)
  $ tree
  .
  `-- syncinfo
  
  0 directories, 1 file

  $ josh-sync --file syncinfo -m "sync libraries"

  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file3
  |-- c
  |   |-- file1
  |   `-- file2
  `-- syncinfo
  
  3 directories, 4 files

  $ git ls-files --with-tree=HEAD
  a/b/file3
  c/file1
  c/file2
  syncinfo

  $ git status
  On branch master
  nothing to commit, working tree clean

  $ git log | sed -e 's/[ ]*$//g'
  commit * (glob)
  Author: * (glob)
  Date: * (glob)
  
      sync libraries
  
      Synced: libs(master) rev: * (glob)
      Synced: libs(foo) rev: * (glob)
  
  commit * (glob)
  Author: * (glob)
  Date: * (glob)
  
      initial


  $ cat > syncinfo <<EOF
  > [libs(master)]
  > d/f/g = :/sub1
  > [libs(foo)]
  > xx = :/sub2
  > EOF

  $ josh-sync --file syncinfo -m "sync libraries"

  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file3
  |-- c
  |   |-- file1
  |   `-- file2
  |-- d
  |   `-- f
  |       `-- g
  |           |-- file1
  |           `-- file2
  |-- syncinfo
  `-- xx
      `-- file3
  
  7 directories, 7 files

  $ git ls-files --with-tree=HEAD
  a/b/file3
  c/file1
  c/file2
  d/f/g/file1
  d/f/g/file2
  syncinfo
  xx/file3

  $ git status
  On branch master
  Changes to be committed:
    (use "git restore --staged <file>..." to unstage)
  \tmodified:   syncinfo (esc)
  
  Changes not staged for commit:
    (use "git add <file>..." to update what will be committed)
    (use "git restore <file>..." to discard changes in working directory)
  \tmodified:   syncinfo (esc)
  


  $ git log | sed -e 's/[ ]*$//g'
  commit * (glob)
  Author: * (glob)
  Date:   * (glob)
  
      sync libraries
  
      Synced: libs(master) rev: * (glob)
      Synced: libs(foo) rev: * (glob)
  
  commit * (glob)
  Author: * (glob)
  Date: * (glob)
  
      sync libraries
  
      Synced: libs(master) rev: * (glob)
      Synced: libs(foo) rev: * (glob)
  
  commit * (glob)
  Author: * (glob)
  Date: * (glob)
  
      initial

