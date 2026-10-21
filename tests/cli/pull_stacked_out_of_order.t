Out-of-order landing: with a local stack A (HEAD) -> B -> C on master D and
three open PRs, upstream merges only the middle change B. The pull must skip B
(Change-Id match) and restack C and A on top of the landed B, ending up with
A' -> C' -> B(landed) -> D.

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

  $ josh --store filtered.store clone ${TESTTMP}/remote filtered --mount origin --filter :/sub1 2>&1 | tail -2 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added remote 'origin' at 'origin'
  Cloned worktree to ${TESTTMP}/filtered
  $ cd filtered
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Make a stack of three changes (C oldest, A newest)

  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "change C" -m "Change-Id: 1111"
  $ echo "contents3" > file3
  $ git add file3
  $ git commit -q -m "change B" -m "Change-Id: 2222"
  $ echo "contents4" > file4
  $ git add file4
  $ git commit -q -m "change A" -m "Change-Id: 3333"

Publish the stack (three open PRs)

  $ josh changes publish
  published 3 changes (3 new)

Emulate the merge queue landing ONLY the middle change B on master
(the landed commit keeps the Change-Id trailer)

  $ cd ${TESTTMP}/local
  $ echo "contents3" > sub1/file3
  $ git add sub1/file3
  $ git commit -q -m "landed change B" -m "Change-Id: 2222"
  $ git push -q origin master
  $ cd ..

Pull: B is skipped, C and A are restacked on top of the landed B

  $ cd ${TESTTMP}/filtered
  $ josh changes pull
  Fetched from remote: origin
  Skipping 1 already applied change: 2222
  Restacked master: kept 2 changes
  $ git log --graph --pretty="%s"
  * change A
  * change C
  * landed change B
  * add file1
  $ git-tree-pretty .
  .
  ├── file1
  │   ┆  file1 content
  ├── file2
  │   ┆  contents2
  ├── file3
  │   ┆  contents3
  └── file4
      ┆  contents4

Pull again: no-op

  $ josh changes pull
  Fetched from remote: origin
  Already up to date.
