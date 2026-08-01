KNOWN BROKEN: a local change that is a merge commit (e.g. vendoring: one
merge commit with a Change-Id trailer, bringing in unrelated history) cannot
be restacked by `josh changes pull`. When upstream advances, the pull should
recreate the merge on top of the new upstream tip; today it bails out with
"contains merge commits". This test documents the current broken behavior.

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

Vendor an unrelated history into the branch as a single merge-commit change
(orphan vendor branch merged with --no-ff, Change-Id trailer on the merge)

  $ git checkout -q --orphan vendor
  $ git rm -q -rf .
  $ echo "lib v1" > lib.txt
  $ git add lib.txt
  $ git commit -q -m "vendor: initial lib"
  $ git checkout -q master
  $ git merge -q --no-ff --allow-unrelated-histories vendor -m "vendor the lib" -m "Change-Id: 4444"

  $ git log --graph --pretty="%s"
  *   vendor the lib
  |\  
  | * vendor: initial lib
  * add file1

Upstream advances with an unrelated commit

  $ cd ${TESTTMP}/local
  $ echo "other" > sub1/file9
  $ git add sub1/file9
  $ git commit -q -m "unrelated upstream commit"
  $ git push -q origin master

Pull: the merge-commit change cannot be restacked automatically; the pull
bails out and leaves refs and worktree untouched

  $ cd ${TESTTMP}/filtered
  $ git rev-parse master > ${TESTTMP}/before
  $ josh changes pull
  updated master (5f2928c..49dc0ec)
  Error: local branch 'master' contains merge commits; cannot integrate automatically
  local branch 'master' contains merge commits; cannot integrate automatically
  [1]
  $ git rev-parse master > ${TESTTMP}/after
  $ diff ${TESTTMP}/before ${TESTTMP}/after
  $ git show -s --format=%s master
  vendor the lib
  $ test "$(git show -s --format=%P master | wc -w)" = "2" && echo merge-preserved
  merge-preserved
  $ git status --porcelain
  $ ls
  file1
  lib.txt
