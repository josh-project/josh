  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

  $ cd ${TESTTMP}
  $ git init testrepo &>/dev/null
  $ cd testrepo

  $ echo contents1 > testfile
  $ git add testfile
  $ git commit --cleanup=verbatim -m '
  > #2345346
  > ' -m "blabla" &>/dev/null

  $ josh-filter master c=:prefix=pre
  $ git cat-file commit master
  tree * (glob)
  author * (glob)
  committer * (glob)
  
  
  #2345346
  
  blabla
  $ git cat-file commit josh/filter/master
  tree * (glob)
  author * (glob)
  committer * (glob)
  
  
  #2345346
  
  blabla

