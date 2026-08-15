  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents3 > sub1/file3
  $ git add sub1
  $ git commit -m "add sub1" 1> /dev/null

  $ mkdir sub2
  $ echo contents2 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

select keeps only the paths matched by the inner filter (the complement of exclude):

  $ josh-filter -s :select[::sub2/] master --update refs/heads/selected
  02bc56535aa5b74aa2b6de2a1bb66fbc465f8ca2
  [2] :select[::sub2/]
  [2] reachable_roots
  [2] sequence_number
  $ git checkout selected 1> /dev/null
  Switched to branch 'selected'
  $ tree
  .
  `-- sub2
      `-- file2
  
  2 directories, 1 file

  $ cat sub2/file2
  contents2

selecting into a subtree keeps only the matched file, dropping siblings and other trees:

  $ git checkout master 1> /dev/null
  Switched to branch 'master'
  $ josh-filter :select[::sub1/file1] master --update refs/heads/selected_file
  179c05ebefb1e481a31334cdf6dc552e28e28151
  $ git checkout selected_file 1> /dev/null
  Switched to branch 'selected_file'
  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file


:select[::sub2/] and :exclude[::sub1/] produce the same tree:

  $ git checkout master 1> /dev/null
  Switched to branch 'master'
  $ josh-filter :exclude[::sub1/] master --update refs/heads/excluded
  02bc56535aa5b74aa2b6de2a1bb66fbc465f8ca2
  $ test $(git rev-parse selected) = $(git rev-parse excluded) && echo SAME
  SAME
