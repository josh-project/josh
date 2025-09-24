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
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@changes/master/josh@example.com/1234
  Pushed c1b55ea7e5f27f82d3565c1f5d64113adf635c2c to origin/refs/heads/@changes/master/josh@example.com/foo7
  Pushed ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9 to origin/refs/heads/@changes/master/josh@example.com/1235
  Pushed 02796cbb12e05f3be9f16c82e4d26542af7e700c to origin/refs/heads/@heads/master/josh@example.com

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git ls-remote . | grep "@" | sort
  02796cbb12e05f3be9f16c82e4d26542af7e700c\trefs/heads/@heads/master/josh@example.com (esc)
  c1b55ea7e5f27f82d3565c1f5d64113adf635c2c\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9\trefs/heads/@changes/master/josh@example.com/1235 (esc)

Test that we can fetch the split refs back

  $ cd ${TESTTMP}/filtered
  $ git fetch origin

  $ git log --all --decorate --graph --pretty="%s %d"
  * Change-Id: 1235  (HEAD -> master)
  * Change-Id: foo7 
  * Change-Id: 1234 
  * add file1  (origin/master, origin/HEAD)
  * Change-Id: 1235 
  | * Change-Id: foo7 
  | | * Change-Id: 1235 
  | | * Change-Id: foo7 
  | |/  
  |/|   
  * | Change-Id: 1234 
  |/  
  * add file1 

Test normal push still works

  $ echo "contents4" > file2
  $ git add file2
  $ git commit -q -m "add file4" -m "Change-Id: 1236"
  $ josh push
  Pushed 84f0380f63011c5432945683f8f79426cc6bd180 to origin/master

Verify normal push worked

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file2
  contents4
