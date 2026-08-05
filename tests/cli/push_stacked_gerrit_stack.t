Publishing to a Gerrit remote in `stack` mode (opted in via --gerrit-mode): the
whole commit history is pushed once to refs/for/master as a single relation
chain -- one change per commit, each with exactly one parent. A dry-run is used
so the plain bare repo does not need to accept the magic ref.

  $ export TESTTMP=${PWD}

  $ git init -q --bare upstream
  $ git init -q seed
  $ cd seed
  $ mkdir -p sub1
  $ echo "file1 content" > sub1/file1
  $ git add .
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/upstream
  $ git push -q origin master
  $ cd ..

Clone selecting the Gerrit forge and the `stack` publish mode. The mode is
stored per-remote in the remote config.

  $ josh clone ${TESTTMP}/upstream :/sub1 filtered --forge gerrit --gerrit-mode stack > /dev/null 2>&1
  $ cd filtered
  $ grep gerrit-mode "$(git rev-parse --git-common-dir)/josh/remotes/origin.josh"
      gerrit-mode="stack"
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

  $ echo "aaa" > fileA
  $ git add fileA
  $ printf "Add fileA\n\nChange: alpha\nChange-Series: dep1" | git commit -q -F -
  $ echo "bbb" > fileB
  $ git add fileB
  $ printf "Add fileB\n\nChange: beta" | git commit -q -F -
  $ echo "ccc" > fileC
  $ git add fileC
  $ printf "Add fileC\n\nChange: gamma\nChange-Series: dep1" | git commit -q -F -

Publish: the entire stack goes to refs/for/master in a single push.

  $ josh changes publish --dry-run
  Pushing 4b0fa539289928ceea5f36fa8d7c5b7688b4b0eb to origin/refs/for/master
  To file://${TESTTMP}/upstream
   * [new reference]   4b0fa539289928ceea5f36fa8d7c5b7688b4b0eb -> refs/for/master
  
  Pushed 1 ref(s) to origin

The pushed tip is one relation chain over the target branch: every change
appears exactly once with a single parent, each carrying a Gerrit Change-Id.

  $ git log --pretty="%s | %(trailers:key=Change-Id,valueonly,separator=%x20)" 4b0fa539289928ceea5f36fa8d7c5b7688b4b0eb ^refs/josh/remotes/origin/master
  Add fileC | If6ce3c605d8e619e80eb03eb65fc984a940abde7
  Add fileB | Ie1d65540f4fdf72431ec47e001282cc7e8ed7c0c
  Add fileA | I7e74e68b2a782a3aead46d987a63ca1c91091c13
