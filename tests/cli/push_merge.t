upstream merge commit.
  $ export TESTTMP=${PWD}


  $ git init -q --bare upstream


  $ git init -q seed


  $ echo base > seed/file.txt


  $ git -C seed add file.txt


  $ git -C seed commit -q -m base


  $ git -C seed remote add origin ${TESTTMP}/upstream


  $ git -C seed push -q origin master


  $ josh init store >/dev/null


  $ cd store


  $ josh remote add project ${TESTTMP}/upstream 2>/dev/null


  $ josh worktree add work :/project 2>/dev/null >/dev/null


  $ cd work


  $ echo change >> file.txt


  $ git commit -q -am change


  $ josh push project HEAD:refs/heads/merged --base=master --merge >/dev/null 2>&1


  $ git --git-dir=${TESTTMP}/upstream rev-list --parents -n 1 merged | wc -w | tr -d ' '
  3


  $ git --git-dir=${TESTTMP}/upstream log -1 --format='%s' merged
  Merge from :/


  $ git --git-dir=${TESTTMP}/upstream show merged:file.txt
  base
  change


`--merge` without an existing destination or explicit base is rejected.
  $ josh push project HEAD:refs/heads/other --merge
  Error: --merge requires --base=<ref> or an existing destination ref
  --merge requires --base=<ref> or an existing destination ref
  [1]
