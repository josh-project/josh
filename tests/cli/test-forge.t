The test forge is a test-suite-only forge variant: `--forge test` round-trips
through remote and link config like any other forge, but it is never guessed
from a URL.

  $ export TESTTMP=${PWD}

Set up a remote repository and a work repo.

  $ git init -q remote
  $ cd remote
  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -q -m "add file1"
  $ cd ${TESTTMP}

  $ git init -q work
  $ cd work

A remote records the test forge as a meta key in its config file.

  $ josh remote add origin ${TESTTMP}/remote :/sub1 --forge test
  Added remote 'origin' with filter ':/sub1'

  $ cat .git/josh/remotes/origin.josh | sed "s|file://.*/remote|file://\${TESTTMP}/remote|"
  :~(
      fetch="+refs/heads/*:refs/josh/remotes/origin/*"
      forge="test"
      url="file://${TESTTMP}/remote"
  )[
      :/sub1
  ]

A link records the test forge the same way (links are experimental).

  $ export JOSH_EXPERIMENTAL_FEATURES=1
  $ josh link add ${TESTTMP}/remote :/sub1 --forge test | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added link 'remote': ${TESTTMP}/remote refs/heads/master :/sub1

  $ git show refs/josh/links/remote:link.josh | sed "s|${TESTTMP}|\${TESTTMP}|g"
  :~(
      forge="test"
      tracked-ref="refs/heads/master"
      url="${TESTTMP}/remote"
  )[
      :/sub1
  ]

  $ josh link list | sed "s|${TESTTMP}|\${TESTTMP}|g"
  remote\t${TESTTMP}/remote\trefs/heads/master\ttest\t:/sub1 (escaped)

The hidden `josh forge` command writes the test forge's server-side state
(CI checks, reviews, maintainers, admission rules) into a standalone ref in
the remote repository.

Only test-forge remotes are accepted.

  $ josh remote add plain ${TESTTMP}/remote :/sub1
  Added remote 'plain' with filter ':/sub1'
  $ oid=$(git -C ${TESTTMP}/remote rev-parse master)
  $ josh forge --remote plain check set $oid build success
  Error: remote 'plain' is not a test-forge remote (forge: none)
  remote 'plain' is not a test-forge remote (forge: none)
  [1]

Checks attach to a head commit; unknown states and unknown commits are
rejected.

  $ josh forge --remote origin check set $oid build bogus
  Error: unknown check state 'bogus' (valid: pending, success, failure, neutral, skipped, cancelled, timed_out, action_required)
  unknown check state 'bogus' (valid: pending, success, failure, neutral, skipped, cancelled, timed_out, action_required)
  [1]
  $ josh forge --remote origin check set 1111111111111111111111111111111111111111 build success
  Error: commit 1111111111111111111111111111111111111111 not found in the remote repository
  commit 1111111111111111111111111111111111111111 not found in the remote repository
  [1]
  $ josh forge --remote origin review change/1 alice bogus
  Error: unknown review state 'bogus' (valid: approved, changes_requested, commented, dismissed)
  unknown review state 'bogus' (valid: approved, changes_requested, commented, dismissed)
  [1]

Build up the forge state: checks keyed by commit oid, reviews by change-id,
the maintainer set, and per-branch admission rules.

  $ josh forge --remote origin maintainer add alice
  Added maintainer 'alice'
  $ josh forge --remote origin maintainer add bob
  Added maintainer 'bob'
  $ josh forge --remote origin check set $oid build success | sed "s|$oid|OID|"
  Set check 'build' on OID to success
  $ josh forge --remote origin check set $oid test pending | sed "s|$oid|OID|"
  Set check 'test' on OID to pending
  $ josh forge --remote origin review change/1 alice approved
  Recorded review by alice on change 'change/1': approved
  $ josh forge --remote origin admission set --branch master --require-check build --require-check test --required-approvals 2
  Set admission for branch 'master': required checks [build, test], required approvals 2

The state lives in refs/josh/forges/test in the remote repository;
change-ids keep their %2F path escaping.

  $ (cd ${TESTTMP}/remote && git-tree-pretty refs/josh/forges/test) | sed "s|$oid|OID|"
  .
  ├── admission/
  │   └── master/
  │       ├── required_approvals
  │       │   ╵  2
  │       └── required_checks/
  │           ├── build/
  │           └── test/
  ├── checks/
  │   └── OID/
  │       ├── build
  │       │   ╵  success
  │       └── test
  │           ╵  pending
  ├── maintainers/
  │   ├── alice/
  │   └── bob/
  └── reviews/
      └── change%2F1/
          └── alice
              ╵  approved

Admission re-set is wholesale (the check list is replaced, not extended),
and maintainers can be removed.

  $ josh forge --remote origin admission set --branch master --require-check lint
  Set admission for branch 'master': required checks [lint], required approvals 0
  $ josh forge --remote origin maintainer remove bob
  Removed maintainer 'bob'
  $ (cd ${TESTTMP}/remote && git-tree-pretty refs/josh/forges/test) | sed "s|$oid|OID|"
  .
  ├── admission/
  │   └── master/
  │       ├── required_approvals
  │       │   ╵  0
  │       └── required_checks/
  │           └── lint/
  ├── checks/
  │   └── OID/
  │       ├── build
  │       │   ╵  success
  │       └── test
  │           ╵  pending
  ├── maintainers/
  │   └── alice/
  └── reviews/
      └── change%2F1/
          └── alice
              ╵  approved
