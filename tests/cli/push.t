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
  $ echo "file2 content" > sub1/file2
  $ git add sub1
  $ git commit -q -m "add files"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

Clone with josh filter

  $ josh clone ${TESTTMP}/remote:/sub1 filtered
  Successfully added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin
  Successfully cloned repository to: filtered
  $ cd filtered
  $ tree
  .
  |-- file1
  `-- file2
  
  1 directory, 2 files

Make a change in the filtered repository

  $ echo "modified content" > file1
  $ git add file1
  $ git commit -q -m "modify file1"

Push the change back

  $ josh push
  Successfully pushed master to origin/master

Verify the change was pushed to the original repository

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file1
  modified content