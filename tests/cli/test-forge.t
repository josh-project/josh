The test forge is a test-suite-only forge variant: `--forge test` round-trips
through remote and link config like any other forge, but it is never guessed
from a URL. `josh changes sync` pulls its server-side state (written by
`josh forge`) into the changes refs like GitHub data.

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

Sync works against a test-forge remote; with nothing published there is
nothing to do.

  $ echo x > file
  $ git add file
  $ git commit -q -m x
  $ josh changes sync --remote origin
  No published changes found on the test forge.

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

Sync from a test-forge remote: published changes plus the forge state written
by `josh forge` land in the `test/` and `test_admission/` namespaces of the
remote's changes ref. Set up a bare remote and a clone with two changes.

  $ cd ${TESTTMP}
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

  $ josh clone ${TESTTMP}/upstream :/sub1 w2 --forge test > /dev/null 2>&1
  $ cd w2
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh"

  $ echo aaa > fileA
  $ git add fileA
  $ printf "A change\n\nChange: alpha" | git commit -q -F -
  $ echo bbb > fileB
  $ git add fileB
  $ printf "B change\n\nChange: beta" | git commit -q -F -

Publish: even a branch-based (non-GitHub) publish writes the per-change
`@changes/<target>/<author>/<change-id>` refs sync enumerates.

  $ josh changes publish
  published 2 changes (2 new)

Sync with no forge state written yet: succeeds with empty state.

  $ josh changes sync --remote origin
  Found 2 published changes on the test forge.

The changes ref holds a `test/<change-id>` entry per published change (the
`gh/` layout's analogue) and a `test_admission/` subtree for the branch.
Integer fields of the shared PrData/AdmissionData structs are little-endian
binary blobs, so assert structure with --no-contents and spot-check contents
with cat-file.

  $ git-tree-pretty --no-contents refs/josh/remotes/origin/changes/master
  .
  ├── test/
  │   ├── alpha/
  │   │   ├── additions
  │   │   ├── author
  │   │   ├── base_ref_name
  │   │   ├── body
  │   │   ├── changed_files
  │   │   ├── checks/
  │   │   ├── created_at
  │   │   ├── deletions
  │   │   ├── head_ref_name
  │   │   ├── reviews/
  │   │   ├── state
  │   │   ├── title
  │   │   └── updated_at
  │   └── beta/
  │       ├── additions
  │       ├── author
  │       ├── base_ref_name
  │       ├── body
  │       ├── changed_files
  │       ├── checks/
  │       ├── created_at
  │       ├── deletions
  │       ├── head_ref_name
  │       ├── reviews/
  │       ├── state
  │       ├── title
  │       └── updated_at
  └── test_admission/
      ├── fetched_at
      ├── maintainers/
      ├── required_approvals
      └── required_checks/

The stored fields are commit-derived, and the PR-specific fields of
`ChangeData` (number, url, draft/merge state) are absent entirely -- the
test forge has no values for them, and `None` is not stored.

  $ for p in title state author base_ref_name head_ref_name created_at; do printf "%s: " "$p"; git cat-file blob "refs/josh/remotes/origin/changes/master:test/alpha/$p"; echo; done
  title: A change
  state: Open
  author: josh@example.com
  base_ref_name: master
  head_ref_name: @changes/master/josh@example.com/alpha
  created_at: 2005-04-07T22:13:13Z

  $ git ls-tree refs/josh/remotes/origin/changes/master:test/alpha/ | grep -cE "number|url|is_draft|merged" || true
  0

Now write forge state and sync again: checks keyed by the change's head
commit (alpha has one, beta has none), reviews by change-id, the maintainer
set, and the branch's admission rules.

  $ alpha_oid=$(git rev-parse refs/josh/remotes/origin/@changes/master/josh@example.com/alpha)
  $ josh forge --remote origin maintainer add alice
  Added maintainer 'alice'
  $ josh forge --remote origin check set $alpha_oid build success | sed "s|$alpha_oid|OID|"
  Set check 'build' on OID to success
  $ josh forge --remote origin review alpha alice approved
  Recorded review by alice on change 'alpha': approved
  $ josh forge --remote origin review beta alice changes_requested
  Recorded review by alice on change 'beta': changes_requested
  $ josh forge --remote origin admission set --branch master --require-check build --required-approvals 1
  Set admission for branch 'master': required checks [build], required approvals 1

  $ josh changes sync --remote origin
  Found 2 published changes on the test forge.

The fetched forge-state ref lands inside the remote's namespace.

  $ git rev-parse --verify -q refs/josh/remotes/origin/forges/test > /dev/null && echo present
  present

  $ git-tree-pretty --no-contents refs/josh/remotes/origin/changes/master
  .
  ├── test/
  │   ├── alpha/
  │   │   ├── additions
  │   │   ├── author
  │   │   ├── base_ref_name
  │   │   ├── body
  │   │   ├── changed_files
  │   │   ├── checks/
  │   │   │   └── build
  │   │   ├── created_at
  │   │   ├── deletions
  │   │   ├── head_ref_name
  │   │   ├── reviews/
  │   │   │   └── alice
  │   │   ├── state
  │   │   ├── title
  │   │   └── updated_at
  │   └── beta/
  │       ├── additions
  │       ├── author
  │       ├── base_ref_name
  │       ├── body
  │       ├── changed_files
  │       ├── checks/
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

  $ git cat-file blob refs/josh/remotes/origin/changes/master:test/alpha/reviews/alice
  approved (no-eol)
  $ git cat-file blob refs/josh/remotes/origin/changes/master:test/alpha/checks/build
  success (no-eol)
  $ git cat-file blob refs/josh/remotes/origin/changes/master:test/beta/reviews/alice
  changes_requested (no-eol)
  $ git cat-file blob refs/josh/remotes/origin/changes/master:test_admission/required_checks/build/context
  build (no-eol)

required_approvals round-trips from the forge state's decimal string into
AdmissionData's little-endian u32; fetched_at follows JOSH_COMMIT_TIME.

  $ git cat-file blob refs/josh/remotes/origin/changes/master:test_admission/required_approvals | od -An -tu1 | tr -d ' \n'; echo
  1000
  $ git cat-file blob refs/josh/remotes/origin/changes/master:test_admission/fetched_at | od -An -tu1 | tr -d ' \n'; echo
  00000000

--push has no test-forge meaning; --clean rebuilds the changes ref.

  $ josh changes sync --remote origin --push
  Error: --push is not supported for the test forge
  --push is not supported for the test forge
  [1]

  $ josh changes sync --remote origin --clean
  Found 2 published changes on the test forge.
  $ git cat-file blob refs/josh/remotes/origin/changes/master:test/alpha/checks/build
  success (no-eol)
