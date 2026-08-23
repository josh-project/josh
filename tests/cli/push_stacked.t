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
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/filtered/
  $ cd filtered
  $ git-tree-pretty .
  .
  └── file1
      ┆  file1 content

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
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/josh/filtered/bf567e0faf634a663d6cef48145a035e1974ab1d/heads/master (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/josh/remotes/origin/master (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/namespaces/josh-origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/namespaces/josh-origin/refs/heads/master (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/master (esc)
  $ josh changes publish
  published 2 changes (2 new)

  $ git ls-remote .
  da80e49d24d110866ce2ec7a5c21112696fd165b\tHEAD (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/heads/master (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/josh/filtered/bf567e0faf634a663d6cef48145a035e1974ab1d/heads/master (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/josh/remotes/origin/@base/master/josh@example.com/1234 (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/josh/remotes/origin/@base/master/josh@example.com/foo7 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/josh/remotes/origin/@changes/master/josh@example.com/1234 (esc)
  c1b55ea7e5f27f82d3565c1f5d64113adf635c2c\trefs/josh/remotes/origin/@changes/master/josh@example.com/foo7 (esc)
  2cbfa8cb8d9a9f1de029fcba547a6e56c742733f\trefs/josh/remotes/origin/@heads/master/josh@example.com (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/josh/remotes/origin/master (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/namespaces/josh-origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/namespaces/josh-origin/refs/heads/@base/master/josh@example.com/1234 (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/namespaces/josh-origin/refs/heads/@base/master/josh@example.com/foo7 (esc)
  43d6fcc9e7a81452d7343c78c0102f76027717fb\trefs/namespaces/josh-origin/refs/heads/@changes/master/josh@example.com/1234 (esc)
  ecb19ea4b4fbfb6afff253ec719909e80a480a18\trefs/namespaces/josh-origin/refs/heads/@changes/master/josh@example.com/foo7 (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/namespaces/josh-origin/refs/heads/@heads/master/josh@example.com (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/namespaces/josh-origin/refs/heads/master (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/@base/master/josh@example.com/1234 (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/@base/master/josh@example.com/foo7 (esc)
  43d6fcc9e7a81452d7343c78c0102f76027717fb\trefs/remotes/origin/@changes/master/josh@example.com/1234 (esc)
  ecb19ea4b4fbfb6afff253ec719909e80a480a18\trefs/remotes/origin/@changes/master/josh@example.com/foo7 (esc)
  da80e49d24d110866ce2ec7a5c21112696fd165b\trefs/remotes/origin/@heads/master/josh@example.com (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/HEAD (esc)
  5f2928c89c4dcc7f5a8c59ef65734a83620cefee\trefs/remotes/origin/master (esc)

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git ls-remote .
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\tHEAD (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/heads/@base/master/josh@example.com/1234 (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/heads/@base/master/josh@example.com/foo7 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  c1b55ea7e5f27f82d3565c1f5d64113adf635c2c\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
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
  Pushing d3e371f8c637c91b59e05aae1066cf0adbe0da93 to origin/refs/heads/master
  To file://${TESTTMP}/remote
     6ed6c1c..d3e371f  d3e371f8c637c91b59e05aae1066cf0adbe0da93 -> master
  
  Pushed 1 ref(s) to origin

Verify normal push worked

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file2
  contents3
