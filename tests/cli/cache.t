  $ export TESTTMP=${PWD}


  $ git init -q --bare upstream


  $ git init -q seed


  $ mkdir seed/sub


  $ echo contents > seed/sub/file


  $ git -C seed add sub


  $ git -C seed commit -q -m initial


  $ git -C seed remote add origin ${TESTTMP}/upstream


  $ git -C seed push -q origin master


  $ git --git-dir=upstream symbolic-ref HEAD refs/heads/master


Projections populate the shared filtered-ref and distributed-cache stores.
  $ josh init store >/dev/null


  $ cd store


  $ josh remote add source ${TESTTMP}/upstream 2>/dev/null


  $ josh worktree add view :/source/sub 2>/dev/null >/dev/null


  $ git --git-dir=.josh for-each-ref --format='%(refname)' 'refs/josh/filtered/'
  refs/josh/filtered/8c2b8a3839deb7fb00b0a763104bbd023d12eac0/heads/master


  $ josh cache build source --filter :/source/sub
  Built cache for 1 filter(s) on branch 'master' for remote 'source'


  $ git --git-dir=.josh for-each-ref --format='%(refname)' 'refs/josh/cache/' | wc -l | tr -d ' '
  1


Cache refs and the projection's materialized history can be shared through the upstream.
  $ josh cache push source --filter :/source/sub 2>/dev/null


  $ git ls-remote ${TESTTMP}/upstream 'refs/josh/cache/*' | wc -l | tr -d ' '
  1


  $ git ls-remote ${TESTTMP}/upstream 'refs/josh/filtered/*' | wc -l | tr -d ' '
  1


  $ cd ${TESTTMP}


  $ josh init second >/dev/null


  $ cd second


  $ josh remote add source ${TESTTMP}/upstream 2>/dev/null


  $ josh cache fetch source --filter :/source/sub 2>/dev/null


  $ git --git-dir=.josh for-each-ref --format='%(refname)' 'refs/josh/cache/' | wc -l | tr -d ' '
  1
