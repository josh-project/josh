End-to-end admission scenarios for the test forge: GC of vanished changes,
OID-keyed CI on amend, required-check evolution, maintainer dynamics, and a
multi-change stack. See test-forge.t for the command reference.

  $ export TESTTMP=${PWD}
  $ export JOSH_EXPERIMENTAL_FEATURES=1

Setup: a bare test-forge remote and a clone with two changes.

  $ git init -q --bare upstream
  $ git init -q seed
  $ cd seed
  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/upstream
  $ git push -q origin master
  $ cd ..

  $ josh clone ${TESTTMP}/upstream :/sub1 w --forge test > /dev/null 2>&1
  $ cd w
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh"

  $ echo aaa > fileA
  $ git add fileA
  $ printf "A change\n\nChange: alpha" | git commit -q -F -
  $ echo bbb > fileB
  $ git add fileB
  $ printf "B change\n\nChange: beta" | git commit -q -F -

  $ josh changes publish
  updated master (bb282e9..6ce5921)

Forge state: alpha is green and approved; beta is approved but its check
fails. The branch requires build plus one approval.

  $ alpha_oid=$(git -C ${TESTTMP}/upstream rev-parse master~1)
  $ beta_oid=$(git -C ${TESTTMP}/upstream rev-parse master)
  $ josh forge --remote origin maintainer add alice
  Added maintainer 'alice'
  $ josh forge --remote origin check set $alpha_oid build success | sed "s|$alpha_oid|ALPHA|"
  Set check 'build' on ALPHA to success
  $ josh forge --remote origin check set $beta_oid build failure | sed "s|$beta_oid|BETA|"
  Set check 'build' on BETA to failure
  $ josh forge --remote origin review alpha alice approved
  Recorded review by alice on change 'alpha': approved
  $ josh forge --remote origin review beta alice approved
  Recorded review by alice on change 'beta': approved
  $ josh forge --remote origin admission set --branch master --require-check build --required-approvals 1
  Set admission for branch 'master': required checks [build], required approvals 1
  $ josh changes sync --remote origin
  Found 2 published changes on the test forge.

Multi-change stack: per-change state is independent -- alpha admissible,
beta not (a failing check does not pass).

  $ josh changes list --remote origin
  Changes on remote 'origin' [master]:
  
  6ce5921  beta   D=  1  C=  0  V=      M=no   B change
  2dba34e  alpha  D=  0  C=  0  V=      M=yes  A change
  $ josh changes show --remote origin beta | sed -n 5,8p
  PR:        B change [Open]
  Admission: not admissible
    approved by: alice
    unmet checks: build

Amend resets CI: with beta's check green it is admissible, but amending the
change gives it a new head commit, and checks are keyed by commit OID.

  $ josh forge --remote origin check set $beta_oid build success | sed "s|$beta_oid|BETA|"
  Set check 'build' on BETA to success
  $ josh changes sync --remote origin
  Found 2 published changes on the test forge.
  $ echo "bbb v2" > fileB
  $ git add fileB
  $ printf "B change\n\nChange: beta" | git commit -q --amend -F -
  $ josh changes publish
  force-updated master (6ce5921..52b65e4)
  $ josh changes sync --remote origin
  Found 2 published changes on the test forge.
  $ josh changes list --remote origin
  Changes on remote 'origin' [master]:
  
  52b65e4  beta   D=  1  C=  0  V=      M=no   B change
  2dba34e  alpha  D=  0  C=  0  V=      M=yes  A change
  $ josh changes show --remote origin beta | sed -n 5,8p
  PR:        B change [Open]
  Admission: not admissible
    approved by: alice
    unmet checks: build

The review (keyed by change-id) survived the amend; setting the check on
the new head OID makes the change admissible again.

  $ beta2_oid=$(git -C ${TESTTMP}/upstream rev-parse master)
  $ josh forge --remote origin check set $beta2_oid build success | sed "s|$beta2_oid|BETA2|"
  Set check 'build' on BETA2 to success
  $ josh changes sync --remote origin
  Found 2 published changes on the test forge.
  $ josh changes list --remote origin
  Changes on remote 'origin' [master]:
  
  52b65e4  beta   D=  1  C=  0  V=      M=yes  B change
  2dba34e  alpha  D=  0  C=  0  V=      M=yes  A change

GC: dropping beta's commit and re-publishing moves the branch backwards;
the next sync wipes beta's stored data even though reviews, checks and
admission referenced it, and it disappears from the list.

  $ git reset -q --hard HEAD~1
  $ josh changes publish
  force-updated master (52b65e4..2dba34e)
  $ josh changes sync --remote origin
  Found 1 published changes on the test forge.
  $ git-tree-pretty --no-contents refs/josh/remotes/origin/changes/master
  .
  ├── diffs/
  │   └── alpha/
  │       ├── base
  │       └── commit
  ├── test/
  │   └── alpha/
  │       ├── additions
  │       ├── author
  │       ├── base_ref_name
  │       ├── body
  │       ├── changed_files
  │       ├── checks/
  │       │   └── build
  │       ├── created_at
  │       ├── deletions
  │       ├── head_ref_name
  │       ├── reviews/
  │       │   └── alice
  │       ├── state
  │       ├── title
  │       └── updated_at
  └── test_admission/
      ├── fetched_at
      ├── maintainers/
      │   └── alice/
      ├── required_approvals
      └── required_checks/
          └── build/
              └── context
  $ josh changes list --remote origin
  Changes on remote 'origin' [master]:
  
  2dba34e  alpha  D=  0  C=  0  V=      M=yes  A change

Required-check evolution: adding a requirement blocks; removing it unblocks
and the dropped requirement does not linger in test_admission/.

  $ josh forge --remote origin admission set --branch master --require-check build --require-check test
  Set admission for branch 'master': required checks [build, test], required approvals 0
  $ josh changes sync --remote origin
  Found 1 published changes on the test forge.
  $ git-tree-pretty --no-contents refs/josh/remotes/origin/changes/master:test_admission
  .
  ├── fetched_at
  ├── maintainers/
  │   └── alice/
  ├── required_approvals
  └── required_checks/
      ├── build/
      │   └── context
      └── test/
          └── context
  $ josh changes list --remote origin
  Changes on remote 'origin' [master]:
  
  2dba34e  alpha  D=  0  C=  0  V=      M=no   A change

  $ josh forge --remote origin admission set --branch master --require-check build
  Set admission for branch 'master': required checks [build], required approvals 0
  $ josh changes sync --remote origin
  Found 1 published changes on the test forge.
  $ git-tree-pretty --no-contents refs/josh/remotes/origin/changes/master:test_admission
  .
  ├── fetched_at
  ├── maintainers/
  │   └── alice/
  ├── required_approvals
  └── required_checks/
      └── build/
          └── context

Maintainer dynamics: bob's approval does not count while he is not a
maintainer; adding him makes the same stored review count on the next
sync; removing him flips it back.

  $ josh forge --remote origin review alpha alice commented
  Recorded review by alice on change 'alpha': commented
  $ josh forge --remote origin review alpha bob approved
  Recorded review by bob on change 'alpha': approved
  $ josh changes sync --remote origin
  Found 1 published changes on the test forge.
  $ josh changes show --remote origin alpha | sed -n 5,7p
  PR:        A change [Open]
  Admission: not admissible
  

  $ josh forge --remote origin maintainer add bob
  Added maintainer 'bob'
  $ josh changes sync --remote origin
  Found 1 published changes on the test forge.
  $ josh changes show --remote origin alpha | sed -n 5,7p
  PR:        A change [Open]
  Admission: admissible
    approved by: bob

  $ josh forge --remote origin maintainer remove bob
  Removed maintainer 'bob'
  $ josh changes sync --remote origin
  Found 1 published changes on the test forge.
  $ josh changes list --remote origin
  Changes on remote 'origin' [master]:
  
  2dba34e  alpha  D=  0  C=  0  V=      M=no   A change

GC to an empty branch: dropping the last change clears every stored
namespace.

  $ git reset -q --hard HEAD~1
  $ josh changes publish
  force-updated master (2dba34e..bb282e9)
  $ josh changes sync --remote origin
  No published changes found on the test forge.
  $ josh changes list --remote origin
  No changes found on remote 'origin' [master].
  $ git-tree-pretty --no-contents refs/josh/remotes/origin/changes/master
  .
