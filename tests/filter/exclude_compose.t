  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir sub3
  $ echo contents1 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ josh-filter -s :exclude[:/sub2] master --update refs/heads/hidden
  [2] :/sub2
  [2] :exclude[:/sub2]
  $ git checkout hidden 1> /dev/null
  Switched to branch 'hidden'
  $ tree
  .
  |-- sub1
  |   `-- file1
  `-- sub3
      `-- file3
  
  2 directories, 2 files
  $ git log --graph --pretty=%s
  * add file3
  * add file1

  $ echo contents3 > sub1/file3
  $ git add sub1/file3
  $ git commit -m "add sub1/file3" 1> /dev/null

  $ josh-filter -s :exclude[:/sub1,:/sub2] master --update refs/josh/filtered
  [1] :/sub1
  [2] :/sub2
  [2] :[
      :/sub1
      :/sub2
  ]
  [2] :exclude[
      :/sub1
      :/sub2
  ]
  [2] :exclude[:/sub2]

  $ git checkout refs/josh/filtered
  Note: switching to 'refs/josh/filtered'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at 9085ae2 add file3
  $ tree
  .
  `-- sub3
      `-- file3
  
  1 directory, 1 file
