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

  $ git ls-remote .
  da80e49d24d110866ce2ec7a5c21112696fd165b\tHEAD (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/heads/master (esc)
  3c2c2237ae79b148f5a4ca12279f75ab6029fe2b\trefs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/josh/remotes/origin/master (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/namespaces/josh-origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/namespaces/josh-origin/refs/heads/master (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/master (esc)
  $ josh push --stack
  To file://${TESTTMP}/remote
   * [new branch]      c61c37f4a3d5eb447f41dde15620eee1a181d60b -> @changes/master/josh@example.com/1234
  
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@changes/master/josh@example.com/1234
  To file://${TESTTMP}/remote
   * [new branch]      6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d -> @base/master/josh@example.com/1234
  
  Pushed 6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d to origin/refs/heads/@base/master/josh@example.com/1234
  To file://${TESTTMP}/remote
   * [new branch]      2cbfa8cb8d9a9f1de029fcba547a6e56c742733f -> @changes/master/josh@example.com/foo7
  
  Pushed 2cbfa8cb8d9a9f1de029fcba547a6e56c742733f to origin/refs/heads/@changes/master/josh@example.com/foo7
  To file://${TESTTMP}/remote
   * [new branch]      c61c37f4a3d5eb447f41dde15620eee1a181d60b -> @base/master/josh@example.com/foo7
  
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@base/master/josh@example.com/foo7
  To file://${TESTTMP}/remote
   * [new branch]      2cbfa8cb8d9a9f1de029fcba547a6e56c742733f -> @heads/master/josh@example.com
  
  Pushed 2cbfa8cb8d9a9f1de029fcba547a6e56c742733f to origin/refs/heads/@heads/master/josh@example.com
  $ git ls-remote .
  da80e49d24d110866ce2ec7a5c21112696fd165b\tHEAD (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/heads/master (esc)
  3c2c2237ae79b148f5a4ca12279f75ab6029fe2b\trefs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/josh/remotes/origin/master (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/namespaces/josh-origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/namespaces/josh-origin/refs/heads/master (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/master (esc)

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git ls-remote .
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\tHEAD (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/heads/@base/master/josh@example.com/1234 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@base/master/josh@example.com/foo7 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  2cbfa8cb8d9a9f1de029fcba547a6e56c742733f\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  2cbfa8cb8d9a9f1de029fcba547a6e56c742733f\trefs/heads/@heads/master/josh@example.com (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/heads/master (esc)

Test normal push (without --split) - create a new commit

  $ cd ${TESTTMP}/filtered
  $ echo "contents3" > file2
  $ git add file2
  $ git commit -q -m "add file3" -m "Change-Id: 1235"
  $ git log --graph --pretty=%s:%H
  * add file3:746bd987ef4122f2e6175f81a025ab335cf51b27
  * Change-Id: foo7:da80e49d24d110866ce2ec7a5c21112696fd165b
  * Change-Id: 1234:43d6fcc9e7a81452d7343c78c0102f76027717fb
  * add file1:5f2928c89c4dcc7f5a8c59ef65734a83620cefee
  $ josh push
  To file://${TESTTMP}/remote
     6ed6c1c..d3e371f  d3e371f8c637c91b59e05aae1066cf0adbe0da93 -> master
  
  Pushed d3e371f8c637c91b59e05aae1066cf0adbe0da93 to origin/master

Verify normal push worked

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file2
  contents3
