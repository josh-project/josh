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

  $ josh push origin HEAD:refs/heads/master -f
  To file://${TESTTMP}/remote
     14ecb7c..33f0c00  33f0c009c43980ba5e76995b53f9615a4d880a08 -> master
  
  Pushed 33f0c009c43980ba5e76995b53f9615a4d880a08 to origin/refs/heads/master
  $ josh push
  Everything up-to-date
  
  Pushed 33f0c009c43980ba5e76995b53f9615a4d880a08 to origin/refs/heads/master

Verify the change was pushed to the original repository

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ git log --oneline --graph
  * 33f0c00 modify file1
  * 14ecb7c add file2
  * 115b269 add file1
  $ cat sub1/file1
  modified content

Make a commit on a new local branch in the filtered repo and push it
to a brand-new remote branch using master as the unfiltered base for
reverse filtering. Without --base the unfiltered base would be zero;
--base=master makes the new unfiltered commit a descendant of master.

  $ cd ${TESTTMP}/filtered
  $ git switch -q -c feature
  $ echo "feature content" > file3
  $ git add file3
  $ git commit -q -m "add file3"
  $ josh push origin HEAD:refs/heads/feature --base=master
  To file://${TESTTMP}/remote
  * (glob)
  
  Pushed * to origin/refs/heads/feature (glob)

--base must resolve to an existing remote-tracking ref.

  $ cd ${TESTTMP}/filtered
  $ josh push origin HEAD:refs/heads/other --base=does-not-exist
  Error: Failed to resolve --base ref (looked up 'refs/josh/remotes/origin/does-not-exist')
  Failed to resolve --base ref (looked up 'refs/josh/remotes/origin/does-not-exist')
  * (glob)
  [1]
