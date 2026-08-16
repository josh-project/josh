Publishing to a Gerrit remote in the default `independent` mode: only changes
with no dependencies are pushed to refs/for/master, each as its own independent
review. A change that still depends on unmerged work is skipped until its
dependencies merge. This keeps each change on a single, unambiguous ancestry
(one commit on the target base), so no change is ever duplicated across stacks.
A dry-run is used so the plain bare repo does not need to accept the magic ref.

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

Clone with the Gerrit forge. No --gerrit-mode is given, so the default
(independent) applies.

  $ josh clone ${TESTTMP}/upstream :/sub1 filtered --forge gerrit > /dev/null 2>&1
  $ cd filtered
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

alpha and gamma share Change-Series dep1 (so gamma depends on alpha); beta is
independent.

  $ echo "aaa" > fileA
  $ git add fileA
  $ printf "Add fileA\n\nChange: alpha\nChange-Series: dep1" | git commit -q -F -
  $ echo "bbb" > fileB
  $ git add fileB
  $ printf "Add fileB\n\nChange: beta" | git commit -q -F -
  $ echo "ccc" > fileC
  $ git add fileC
  $ printf "Add fileC\n\nChange: gamma\nChange-Series: dep1" | git commit -q -F -

Publish: only the dependency-free changes (alpha, beta) are pushed, each in its
own push. gamma depends on alpha, so it is skipped this round.

  $ josh changes publish --dry-run
  Pushing e042bde9d87ed49a86e729ab6c936aa349a610bc to origin/refs/for/master
  To file://${TESTTMP}/upstream
   * [new reference]   e042bde9d87ed49a86e729ab6c936aa349a610bc -> refs/for/master
  
  Pushing 18954accacea27a2fe6bd457cca353b9e3b0897e to origin/refs/for/master
  To file://${TESTTMP}/upstream
   * [new reference]   18954accacea27a2fe6bd457cca353b9e3b0897e -> refs/for/master
  
  Pushed 2 ref(s) to origin

Each pushed change is a single commit sitting directly on the target base (an
independent review), with a generated Gerrit Change-Id and its subject.

  $ git log --pretty="%s | %(trailers:key=Change-Id,valueonly,separator=%x20)" e042bde9d87ed49a86e729ab6c936aa349a610bc ^refs/josh/remotes/origin/master
  Add fileA | I7e74e68b2a782a3aead46d987a63ca1c91091c13
  $ git log --pretty="%s | %(trailers:key=Change-Id,valueonly,separator=%x20)" 18954accacea27a2fe6bd457cca353b9e3b0897e ^refs/josh/remotes/origin/master
  Add fileB | Ie1d65540f4fdf72431ec47e001282cc7e8ed7c0c

The upstream repo received nothing (dry-run) and no @changes/@base refs exist.

  $ git ls-remote ${TESTTMP}/upstream
  115b269a011d493259a125fa941fd790b903175f\tHEAD (escaped)
  115b269a011d493259a125fa941fd790b903175f\trefs/heads/master (escaped)
