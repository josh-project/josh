`josh changes list / show / deps` sanity tests.

Setup

  $ export TESTTMP=${PWD}

  $ mkdir remote
  $ cd remote
  $ git init -q --bare -b master
  $ cd ..

Seed a single-commit master, push it, then clone so refs/remotes/origin/master
points at the seed commit. `josh changes sync` (Local) needs that ref so it
knows where the stack starts.

  $ mkdir seed
  $ cd seed
  $ git init -q -b master
  $ echo "seed" > seed.txt
  $ git add seed.txt
  $ git commit -q -m "seed"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

  $ git clone -q ${TESTTMP}/remote local
  $ cd local
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Build a three-commit stack:

  $ echo "first revision of A" > fileA
  $ git add fileA
  $ printf "A change\n\nChange-Id: c1\n" | git commit -q -F -

  $ echo "second revision of A" > fileA
  $ git add fileA
  $ printf "B change\n\nChange-Id: c2\n" | git commit -q -F -

  $ echo "brand new file B" > fileB
  $ git add fileB
  $ printf "C change\n\nChange-Id: c3\n" | git commit -q -F -

Populate the Local changes ref (refs/josh/changes/master).

  $ josh changes sync

list: one summary row per change, sorted by deps descending. c2 depends on c1
(both edit fileA); c1 and c3 have no deps so they land at the bottom, with
the subject as the tiebreaker.

  $ josh changes list
  Changes on Local [master]:
  
  c2  D=  1  C=  0  V=      B change
  c1  D=  0  C=  0  V=      A change
  c3  D=  0  C=  0  V=      C change

Add two private comments to c1. The C= column for c1 should pick them up.

  $ josh changes comment c1 -m "hello on c1"
  Comment saved (private to local ref).
  $ josh changes comment c1 -m "another comment"
  Comment saved (private to local ref).

  $ josh changes list
  Changes on Local [master]:
  
  c2  D=  1  C=  0  V=      B change
  c1  D=  0  C=  2  V=      A change
  c3  D=  0  C=  0  V=      C change

deps: c2 depends on c1; c1 and c3 depend on nothing on the ref.

  $ josh changes deps c2
  Depends on:
    c1  A change

  $ josh changes deps c1
  c1 has no dependencies on stored changes.

  $ josh changes deps c3
  c3 has no dependencies on stored changes.

show: full detail for c1 including its two comments. SHA stays a glob; the
comment-line timestamp uses `repo.signature()` at write time, which honors
the GIT_*_DATE env, but the per-comment author/time is later resolved from
the ref's history walk so we glob it too for robustness.

  $ josh changes show c1
  Change-Id: c1
  Commit:    * (glob)
  Author:    josh@example.com
  Date:      2005-04-07 22:13
  
  Subject:   A change
  
  Files (1, +1 / -0):
    +1    -0     fileA
  
  Comments (2):
    [josh@example.com] * (glob)
      hello on c1
    [josh@example.com] * (glob)
      another comment

show: an id with no comments still prints a "Comments (0):" header.

  $ josh changes show c3
  Change-Id: c3
  Commit:    * (glob)
  Author:    josh@example.com
  Date:      2005-04-07 22:13
  
  Subject:   C change
  
  Files (1, +1 / -0):
    +1    -0     fileB
  
  Comments (0):

Error cases: bogus change-ids exit non-zero with a message naming the id and
the resolved scope.

  $ josh changes show bogus 2>&1
  Error: change-id 'bogus' not found on Local [master]
  change-id 'bogus' not found on Local [master]
  [1]

  $ josh changes deps bogus 2>&1
  Error: change-id 'bogus' not found on Local [master]
  change-id 'bogus' not found on Local [master]
  [1]
