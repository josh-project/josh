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
  From $TESTTMP/remote
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/filtered
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: filtered
  $ cd filtered
  $ tree
  .
  `-- file1
  
  1 directory, 1 file

Make multiple changes with Change-Ids for split testing

  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "Change-Id: 1234"
  $ echo "contents2" > file7
  $ git add file7
  $ git commit -q -m "Change-Id: foo7"
  $ echo "contents3" > file2
  $ git add file2
  $ git commit -q -m "Change-Id: 1235"
  $ git log --decorate --graph --pretty="%s %d"
  * Change-Id: 1235  (HEAD -> master)
  * Change-Id: foo7 
  * Change-Id: 1234 
  * add file1  (origin/master, origin/HEAD)

Set up git config for author

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Push with split mode (should create multiple refs for each change)

  $ josh push --split
  To file://$TESTTMP/filtered
     5f2928c..3faa5b5  master -> master
  
  To $TESTTMP/remote
   * [new branch]      c61c37f4a3d5eb447f41dde15620eee1a181d60b -> @changes/master/josh@example.com/1234
  
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@changes/master/josh@example.com/1234
  To $TESTTMP/remote
   * [new branch]      c1b55ea7e5f27f82d3565c1f5d64113adf635c2c -> @changes/master/josh@example.com/foo7
  
  Pushed c1b55ea7e5f27f82d3565c1f5d64113adf635c2c to origin/refs/heads/@changes/master/josh@example.com/foo7
  To $TESTTMP/remote
   * [new branch]      ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9 -> @changes/master/josh@example.com/1235
  
  Pushed ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9 to origin/refs/heads/@changes/master/josh@example.com/1235
  To $TESTTMP/remote
   * [new branch]      02796cbb12e05f3be9f16c82e4d26542af7e700c -> @heads/master/josh@example.com
  
  Pushed 02796cbb12e05f3be9f16c82e4d26542af7e700c to origin/refs/heads/@heads/master/josh@example.com

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git ls-remote . | grep "@" | sort
  02796cbb12e05f3be9f16c82e4d26542af7e700c\trefs/heads/@heads/master/josh@example.com (esc)
  c1b55ea7e5f27f82d3565c1f5d64113adf635c2c\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9\trefs/heads/@changes/master/josh@example.com/1235 (esc)

  $ git log --all --decorate --graph --pretty="%s %d %H"
  * Change-Id: 1235  (@changes/master/josh@example.com/1235) ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9
  | * Change-Id: foo7  (@changes/master/josh@example.com/foo7) c1b55ea7e5f27f82d3565c1f5d64113adf635c2c
  | | * Change-Id: 1235  (@heads/master/josh@example.com) 02796cbb12e05f3be9f16c82e4d26542af7e700c
  | | * Change-Id: foo7  2cbfa8cb8d9a9f1de029fcba547a6e56c742733f
  | |/  
  |/|   
  * | Change-Id: 1234  (@changes/master/josh@example.com/1234) c61c37f4a3d5eb447f41dde15620eee1a181d60b
  |/  
  * add file1  (HEAD -> master) 6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d

Test that we can fetch the split refs back

  $ cd ${TESTTMP}/filtered
  $ josh fetch
  From $TESTTMP/remote
   * [new branch]      @changes/master/josh@example.com/1234 -> refs/josh/remotes/origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/1235 -> refs/josh/remotes/origin/@changes/master/josh@example.com/1235
   * [new branch]      @changes/master/josh@example.com/foo7 -> refs/josh/remotes/origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> refs/josh/remotes/origin/@heads/master/josh@example.com
  
  From file://$TESTTMP/filtered
   + 3faa5b5...5f2928c master     -> origin/master  (forced update)
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/1235 -> origin/@changes/master/josh@example.com/1235
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com
  
  Fetched from remote: origin

  $ git log --all --decorate --graph --pretty="%s %d %H"
  * Change-Id: 1235  (HEAD -> master, origin/@heads/master/josh@example.com) 3faa5b51d4600be54a2b32e84697e7b32a781a03
  * Change-Id: foo7  da80e49d24d110866ce2ec7a5c21112696fd165b
  | * Change-Id: 1235  ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9
  | | * Change-Id: foo7  c1b55ea7e5f27f82d3565c1f5d64113adf635c2c
  | | | * Change-Id: 1235  02796cbb12e05f3be9f16c82e4d26542af7e700c
  | | | * Change-Id: foo7  2cbfa8cb8d9a9f1de029fcba547a6e56c742733f
  | | |/  
  | |/|   
  | * | Change-Id: 1234  c61c37f4a3d5eb447f41dde15620eee1a181d60b
  | |/  
  | * add file1  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d
  | * Change-Id: 1235  (origin/@changes/master/josh@example.com/1235) 96da92a9021ee186e1e9dd82305ddebfd1153ed5
  |/  
  * Change-Id: 1234  (origin/@changes/master/josh@example.com/1234) 43d6fcc9e7a81452d7343c78c0102f76027717fb
  | * Change-Id: foo7  (origin/@changes/master/josh@example.com/foo7) ecb19ea4b4fbfb6afff253ec719909e80a480a18
  |/  
  * add file1  (origin/master, origin/HEAD) 5f2928c89c4dcc7f5a8c59ef65734a83620cefee

Test normal push still works

  $ echo "contents4" > file2
  $ git add file2
  $ git commit -q -m "add file4" -m "Change-Id: 1236"
  $ josh push
  To file://$TESTTMP/filtered
     5f2928c..60b1f76  master -> master
  
  To $TESTTMP/remote
     6ed6c1c..84f0380  84f0380f63011c5432945683f8f79426cc6bd180 -> master
  
  Pushed 84f0380f63011c5432945683f8f79426cc6bd180 to origin/master

Verify normal push worked

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file2
  contents4
