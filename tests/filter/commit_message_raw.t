  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init testrepo 1> /dev/null
  $ cd testrepo

  $ echo contents1 > testfile
  $ git add testfile
  $ git commit --cleanup=verbatim -m '
  > #2345346
  > ' -m "blabla" 1> /dev/null

  $ josh-filter -s c=:prefix=pre master --update refs/josh/filter/master
  [1] :prefix=c
  [1] :prefix=pre
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

