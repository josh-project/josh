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
  $ echo "before" > file7
  $ git add .
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

Clone with josh filter

  $ josh clone ${TESTTMP}/remote :/sub1 filtered
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  Fetched from remote: origin
  Pulled from remote: origin
  Cloned repository to: filtered
  $ cd filtered
  $ tree
  .
  `-- file1
  
  1 directory, 1 file

Make changes with Change-Id for stacked changes

  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "Change-Id: 1234"
  $ echo "contents2" > file7
  $ git add file7
  $ git commit -q -m "Change-Id: foo7"
  $ git log --decorate --graph --pretty="%s %d"
  * Change-Id: foo7  (HEAD -> master)
  * Change-Id: 1234 
  * add file1  (origin/master, origin/HEAD)

Set up git config for author

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Push with stacked changes (should create multiple refs)

  $ josh push --stack
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@changes/master/josh@example.com/1234
  Pushed 2cbfa8cb8d9a9f1de029fcba547a6e56c742733f to origin/refs/heads/@changes/master/josh@example.com/foo7
  Pushed 2cbfa8cb8d9a9f1de029fcba547a6e56c742733f to origin/refs/heads/@heads/master/josh@example.com

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git ls-remote . | grep "@"
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  2cbfa8cb8d9a9f1de029fcba547a6e56c742733f\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  2cbfa8cb8d9a9f1de029fcba547a6e56c742733f\trefs/heads/@heads/master/josh@example.com (esc)

Test normal push (without --split) - create a new commit

  $ cd ${TESTTMP}/filtered
  $ echo "contents3" > file2
  $ git add file2
  $ git commit -q -m "add file3" -m "Change-Id: 1235"
  $ josh push
  Pushed d3e371f8c637c91b59e05aae1066cf0adbe0da93 to origin/master

Verify normal push worked

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file2
  contents3
