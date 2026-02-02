Setup

  $ export TESTTMP=${PWD}

Create a test repository with some content

  $ mkdir remote
  $ cd remote
  $ git init -q --bare
  $ cd ..

  $ mkdir local
  $ cd local
  $ git init -q
  $ mkdir -p sub1
  $ echo "file1 content" > sub1/file1
  $ git add sub1
  $ git commit -q -m "add file1"
  $ echo "file2 content" > sub1/file2
  $ git add sub1
  $ git commit -q -m "add file2"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

Clone with josh filter

  $ josh clone ${TESTTMP}/remote :/sub1 filtered
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/filtered
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/filtered
  $ cd filtered
  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

  $ git log --oneline
  33cbd5f add file2
  5f2928c add file1

Make a change in the filtered repository

  $ echo "modified content" > file1
  $ git add file1
  $ git commit -q -m "modify file1"

Push the change back

  $ josh push -r origin -R HEAD:refs/heads/master -f
  To file://${TESTTMP}/remote
   + 14ecb7c...fc53cf6 fc53cf6782dd7248e98efc9c179c43656a6aa841 -> master (forced update)
  
  Pushed fc53cf6782dd7248e98efc9c179c43656a6aa841 to origin/refs/heads/master
  $ josh push
  To file://${TESTTMP}/remote
   ! [rejected]        33f0c009c43980ba5e76995b53f9615a4d880a08 -> master (non-fast-forward)
  error: failed to push some refs to 'file://${TESTTMP}/remote'
  hint: Updates were rejected because the tip of your current branch is behind
  hint: its remote counterpart. If you want to integrate the remote changes,
  hint: use 'git pull' before pushing again.
  hint: See the 'Note about fast-forwards' in 'git push --help' for details.
  
  Error: git push failed
  git push failed
  Command exited with code 1: git push file:///tmp/prysk-tests-bptaja24/push.t/remote 33f0c009c43980ba5e76995b53f9615a4d880a08:master
  [1]

Verify the change was pushed to the original repository

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ git log --oneline --graph
  * fc53cf6 modify file1
  $ git log --oneline --graph
  * fc53cf6 modify file1
  $ cat sub1/file1
  modified content
