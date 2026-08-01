Setup

  $ export TESTTMP=${PWD}

Create a bare remote and seed it with some content

  $ mkdir remote
  $ cd remote
  $ git init -q --bare
  $ cd ..

  $ mkdir local
  $ cd local
  $ git init -q
  $ mkdir -p sub1
  $ echo "file1 content" > sub1/file1
  $ git add .
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

Clone with josh filter

  $ josh clone ${TESTTMP}/remote :/sub1 filtered 2>&1 | tail -2
  
  Cloned repository to: ${TESTTMP}/filtered/
  $ cd filtered
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Make a stack of three changes

  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "change 2" -m "Change-Id: 1111"
  $ echo "contents3" > file3
  $ git add file3
  $ git commit -q -m "change 3" -m "Change-Id: 2222"
  $ echo "contents4" > file4
  $ git add file4
  $ git commit -q -m "change 4" -m "Change-Id: 3333"

  $ josh changes publish 2>&1 | tail -1
  Pushed 7 ref(s) to origin

Emulate the merge queue landing the first two changes on master
(landed commits keep the Change-Id trailer but differ from the local commits)

  $ cd ${TESTTMP}/local
  $ echo "contents2" > sub1/file2
  $ git add sub1/file2
  $ git commit -q -m "landed change 2" -m "Change-Id: 1111"
  $ echo "contents3" > sub1/file3
  $ git add sub1/file3
  $ git commit -q -m "landed change 3" -m "Change-Id: 2222"
  $ git push -q origin master
  $ cd ..

Pull: applied changes are skipped, the remaining change is restacked

  $ cd ${TESTTMP}/filtered
  $ josh changes pull
  updated master (5f2928c..e19e334)
  Skipping 2 already applied changes: 1111, 2222
  Restacked master: kept 1 change
  $ git log --graph --pretty="%s"
  * change 4
  * landed change 3
  * landed change 2
  * add file1
  $ tree
  .
  |-- file1
  |-- file2
  |-- file3
  `-- file4
  
  1 directory, 4 files

Pull again: no-op

  $ josh changes pull
  Already up to date.

Bail out on diverged commit without a Change-Id

  $ echo "contents5" > file5
  $ git add file5
  $ git commit -q -m "no change id"
  $ cd ${TESTTMP}/local
  $ echo "other" > sub1/file9
  $ git add sub1/file9
  $ git commit -q -m "unrelated upstream commit"
  $ git push -q origin master
  $ cd ${TESTTMP}/filtered
  $ git rev-parse master > before
  $ josh changes pull
  updated master (e19e334..649a1fa)
  Error: local commit 8e601d3 has no Change-Id and history has diverged; cannot integrate automatically
  local commit 8e601d3 has no Change-Id and history has diverged; cannot integrate automatically
  [1]
  $ git rev-parse master > after
  $ diff before after
