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
  tree 2f407f8ecb16a66b85e2c84d3889720b7a0e3762
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  
  
  #2345346
  
  blabla
  $ git cat-file commit josh/filter/master
  tree aba8d3ab2a35a89336235a9ed8222d4bfd9b1843
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  
  
  #2345346
  
  blabla

