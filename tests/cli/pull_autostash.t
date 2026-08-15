Autostash: uncommitted changes survive a pull that touches the same file

  $ export TESTTMP=${PWD}

  $ mkdir remote
  $ cd remote
  $ git init -q libs 1> /dev/null
  $ cd libs
  $ mkdir sub1
  $ printf 'line1\nline2\nline3\n' > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ cd ${TESTTMP}

  $ josh clone ${TESTTMP}/remote/libs :/sub1 libs 2>&1 | tail -2
  
  Cloned repository to: ${TESTTMP}/libs/

Upstream changes the first line of file1

  $ cd ${TESTTMP}/remote/libs
  $ printf 'line1 upstream\nline2\nline3\n' > sub1/file1
  $ git add sub1
  $ git commit -m "update line1" 1> /dev/null

Local uncommitted edit of the last line of the same file

  $ cd ${TESTTMP}/libs
  $ printf 'line1\nline2\nline3 local\n' > file1
  $ git status --porcelain
   M file1

Pull: the edit is stashed, the branch fast-forwards, the edit is re-applied

  $ josh changes pull 2>&1
  updated master (68edcdd..2b3204b)
  Stashed uncommitted changes
  Re-applied stashed changes
  Fast-forwarded master

  $ cat file1
  line1 upstream
  line2
  line3 local
  $ git status --porcelain
   M file1
  $ git stash list
