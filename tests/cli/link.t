  $ export TESTTMP=${PWD}

Set up a remote repository to link to.

  $ git init -q remote
  $ cd remote
  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ cd ${TESTTMP}

  $ josh --store store init >/dev/null
  $ josh --store store remote add upstream ${TESTTMP}/remote >/dev/null 2>&1
  $ josh --store store worktree add work :/upstream >/dev/null 2>&1
  $ cd work
The link feature is experimental.

  $ josh link list
  Error: josh link requires JOSH_EXPERIMENTAL_FEATURES=1
  josh link requires JOSH_EXPERIMENTAL_FEATURES=1
  [1]
  $ export JOSH_EXPERIMENTAL_FEATURES=1

Add a link; the id is derived from the URL and the tracked ref is discovered
from the remote's HEAD symref.

  $ josh link add ${TESTTMP}/remote :/sub1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added link 'remote': ${TESTTMP}/remote refs/heads/master :/sub1

  $ josh link list | sed "s|${TESTTMP}|\${TESTTMP}|g"
  remote\t${TESTTMP}/remote\trefs/heads/master\t\t:/sub1 (escaped)

The link is versioned in its own ref, holding only link.josh.

  $ git log --format='%s' refs/josh/links/remote
  josh link add remote
  $ test "$(git rev-parse refs/josh/links/remote)" = "$(git --git-dir=../store/.josh rev-parse refs/josh/super:links/remote)" && echo authoritative
  authoritative

  $ git show refs/josh/links/remote:link.josh | sed "s|${TESTTMP}|\${TESTTMP}|g"
  :~(
      tracked-ref="refs/heads/master"
      url="${TESTTMP}/remote"
  )[
      :/sub1
  ]

Adding the same link again fails.

  $ josh link add ${TESTTMP}/remote :/sub1
  Error: Link 'remote' already exists
  Link 'remote' already exists
  [1]

An explicit --id overrides the derived one.

  $ josh link add ${TESTTMP}/remote :/sub1 --id custom | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added link 'custom': ${TESTTMP}/remote refs/heads/master :/sub1

  $ josh link list | sed "s|${TESTTMP}|\${TESTTMP}|g"
  custom\t${TESTTMP}/remote\trefs/heads/master\t\t:/sub1 (escaped)
  remote\t${TESTTMP}/remote\trefs/heads/master\t\t:/sub1 (escaped)

Remove links.

  $ josh link rm remote
  Removed link 'remote'

  $ josh link rm remote
  Error: link 'remote' does not exist
  link 'remote' does not exist
  [1]

  $ josh link list | sed "s|${TESTTMP}|\${TESTTMP}|g"
  custom\t${TESTTMP}/remote\trefs/heads/master\t\t:/sub1 (escaped)

  $ git rev-parse --verify -q refs/josh/links/remote
  [1]
  $ git --git-dir=../store/.josh ls-tree refs/josh/super:links | grep -c remote || true
  0

A forge can be recorded explicitly; for a non-GitHub URL the guess is empty.

  $ josh link add ${TESTTMP}/remote :/sub1 --id gh --forge github | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added link 'gh': ${TESTTMP}/remote refs/heads/master :/sub1

  $ git show refs/josh/links/gh:link.josh | sed "s|${TESTTMP}|\${TESTTMP}|g"
  :~(
      forge="github"
      tracked-ref="refs/heads/master"
      url="${TESTTMP}/remote"
  )[
      :/sub1
  ]

  $ josh link add ${TESTTMP}/remote :/sub1 --id nf --no-forge | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added link 'nf': ${TESTTMP}/remote refs/heads/master :/sub1

  $ git show refs/josh/links/nf:link.josh | sed "s|${TESTTMP}|\${TESTTMP}|g"
  :~(
      tracked-ref="refs/heads/master"
      url="${TESTTMP}/remote"
  )[
      :/sub1
  ]

  $ josh link list | sed "s|${TESTTMP}|\${TESTTMP}|g"
  custom\t${TESTTMP}/remote\trefs/heads/master\t\t:/sub1 (escaped)
  gh\t${TESTTMP}/remote\trefs/heads/master\tgithub\t:/sub1 (escaped)
  nf\t${TESTTMP}/remote\trefs/heads/master\t\t:/sub1 (escaped)

--forge and --no-forge conflict.

  $ josh link add ${TESTTMP}/remote :/sub1 --id x --forge github --no-forge
  error: an argument cannot be used with one or more of the other specified arguments
  [2]
