Publishing a stack that contains a merge commit as a change must keep the merge:
the change's @changes branch is a real two-parent merge (not collapsed to a
single-parent change), and its @base branch is the merge's first parent. All
assertions are oid-independent so the test does not depend on the platform's
git version.

  $ export TESTTMP=${PWD}

Bare remote seeded with one file under sub1.

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

Clone through the :/sub1 filter.

  $ josh clone ${TESTTMP}/remote :/sub1 filtered > /dev/null 2>&1
  $ cd filtered
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Build a stack: one ordinary change, then a merge commit carrying a Change-Id.

  $ echo contents2 > file2
  $ git add file2
  $ git commit -q -m "Change-Id: cone"
  $ git checkout -q -b side
  $ echo sidecontents > sidefile
  $ git add sidefile
  $ git commit -q -m "side work"
  $ git checkout -q master
  $ git merge -q --no-ff -m "Change-Id: cmerge" side

The local tip really is a two-parent merge.

  $ git cat-file -p HEAD | grep -c '^parent'
  2

Publish the stack. The remote is a plain path with no forge configured, so this
only creates branches -- no GitHub / PR API calls happen.

  $ josh changes publish > /dev/null 2>&1

Both changes produced @changes and @base branches on the remote.

  $ cd ${TESTTMP}/remote
  $ git for-each-ref --format='%(refname)' | grep -E '/@(changes|base|heads)/' | sort
  refs/heads/@base/master/josh@example.com/cmerge
  refs/heads/@base/master/josh@example.com/cone
  refs/heads/@changes/master/josh@example.com/cmerge
  refs/heads/@changes/master/josh@example.com/cone
  refs/heads/@heads/master/josh@example.com

The ordinary change is a single-parent commit.

  $ git cat-file -p refs/heads/@changes/master/josh@example.com/cone | grep -c '^parent'
  1

The merge change keeps BOTH parents -- it is published as a real merge.

  $ git cat-file -p refs/heads/@changes/master/josh@example.com/cmerge | grep -c '^parent'
  2

Its @base branch is exactly the merge's first parent ...

  $ test "$(git rev-parse refs/heads/@base/master/josh@example.com/cmerge)" = "$(git rev-parse refs/heads/@changes/master/josh@example.com/cmerge^1)" && echo base-is-first-parent
  base-is-first-parent

... and the preserved second parent is a distinct commit.

  $ test "$(git rev-parse refs/heads/@changes/master/josh@example.com/cmerge^2)" != "$(git rev-parse refs/heads/@base/master/josh@example.com/cmerge)" && echo second-parent-distinct
  second-parent-distinct

The first-parent diff (what the PR would show) is the merge's net effect.

  $ git diff --name-only refs/heads/@changes/master/josh@example.com/cmerge^1 refs/heads/@changes/master/josh@example.com/cmerge
  sub1/sidefile
