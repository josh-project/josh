Change-aware fetch summary: a republished change only counts as "updated" when
its content actually changed (stable patch-id), not when it was merely restacked.

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

Clone twice with the same filter and author (second clone acts as a teammate)

  $ josh clone ${TESTTMP}/remote :/sub1 filtered 2>&1 | tail -2
  
  Cloned repository to: ${TESTTMP}/filtered/
  $ cd filtered
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"
  $ cd ..

  $ josh clone ${TESTTMP}/remote :/sub1 filtered2 2>&1 | tail -2
  
  Cloned repository to: ${TESTTMP}/filtered2/
  $ cd filtered2
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"
  $ cd ..

Publish a change from the first clone

  $ cd ${TESTTMP}/filtered
  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "change 2" -m "Change-Id: 1111"
  $ josh changes publish
  published 1 change (1 new)

The teammate fetches the published stack and checks out its tip

  $ cd ${TESTTMP}/filtered2
  $ josh fetch
  updated 1 change
  Fetched from remote: origin
  $ git checkout -q -B master refs/remotes/origin/@heads/master/josh@example.com
  $ git branch --set-upstream-to=origin/master master 1>/dev/null

The teammate amends the change's content (keeping the Change-Id) and republishes

  $ echo "contents2 amended" > file2
  $ git commit -q -a --amend --no-edit
  $ josh changes publish
  published 1 change

Pull in the first clone: the amended change is reported

  $ cd ${TESTTMP}/filtered
  $ josh changes pull
  updated 1 change
  Already up to date.

Upstream advances with an unrelated commit

  $ cd ${TESTTMP}/local
  $ echo "other" > sub1/file9
  $ git add sub1/file9
  $ git commit -q -m "unrelated upstream commit"
  $ git push -q origin master

The teammate restacks onto it (content unchanged) and republishes

  $ cd ${TESTTMP}/filtered2
  $ josh changes pull 2>&1
  updated master (5f2928c..49dc0ec)
  Restacked master: kept 1 change
  $ josh changes publish
  published 1 change

Pull in the first clone: the restacked change has the same patch-id, so no
"updated N changes" line is printed

  $ cd ${TESTTMP}/filtered
  $ josh changes pull 2>&1
  updated master (5f2928c..49dc0ec)
  Restacked master: kept 1 change

  $ git log --pretty="%s"
  change 2
  unrelated upstream commit
  add file1
