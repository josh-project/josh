  $ export TESTTMP=${PWD}

Create two unrelated native repositories and give the first one a branch that
will later be deleted.

  $ git init -q one
  $ mkdir -p one/app
  $ echo one > one/app/file.txt
  $ git -C one add .
  $ git -C one commit -q -m one
  $ git -C one branch gone

  $ git init -q two
  $ mkdir -p two/lib

  $ echo two > two/lib/file.txt
  $ git -C two add .
  $ git -C two commit -q -m two

A store cannot create a checkout until it has an upstream. Remote mounts are
then exposed through isolated, linked repositories.

  $ josh init store >/dev/null
  $ cd store
  $ josh worktree add premature 2>&1 | sed -n '1p'
  Error: Cannot add a worktree before adding a remote
  $ test ! -e premature

  $ josh remote add one ../one 2>/dev/null
  $ josh remote add broken file://${TESTTMP}/missing >/dev/null 2>&1
  [1]
  $ test ! -e .josh/josh/remotes/broken.josh
  $ (cd .josh && git-tree-pretty refs/josh/super:store/remotes) | grep -c broken || true
  0
  $ josh remote add two ../two 2>/dev/null
  $ josh worktree add one-a :/one/app 2>/dev/null >/dev/null
  $ josh worktree add one-b :/one/app 2>/dev/null >/dev/null

An explicit remote-tracking ref selects that remote, including its upstream
configuration, even when another remote sorts first.

  $ josh worktree add two-view :/two --ref refs/remotes/two/master -b selected 2>/dev/null >/dev/null
  $ git -C two-view status --short --branch
  ## selected...two/master
  $ (cd two-view && git-tree-pretty HEAD)
  .
  └── lib/
      └── file.txt
          ┆  two

The synthetic superhistory records projections and active worktree mappings
without exposing its private ref tree in a worktree.

  $ (cd .josh && git-tree-pretty refs/josh/super:store) | grep -c projections/
  1
  $ (cd .josh && git-tree-pretty refs/josh/super:store/worktrees) | grep -c one-a/
  1
  $ test ! -e one-a/changes

Two worktrees using one projection share parked refs but keep their local branches
independent.

  $ git -C one-a checkout -q -b fix
  $ echo parked > one-a/file.txt
  $ git -C one-a add file.txt
  $ git -C one-a commit -q -m parked
  $ git -C one-a push -q one fix
  $ git -C one-b fetch -q one
  $ git -C one-b show one/fix:file.txt
  parked
  $ git -C one-b branch --list fix
  $ git -C one-b checkout -q -b fix one/master
  $ test "$(git -C one-a rev-parse fix)" != "$(git -C one-b rev-parse fix)"

Deleting an upstream branch prunes the native backing ref, every materialized
projection, and registered worktrees.

  $ git -C ../one show-ref --verify --quiet refs/heads/gone
  $ git -C ../one branch -D gone >/dev/null
  $ josh fetch --remote one 2>/dev/null
  $ git -C one-a show-ref --verify --quiet refs/remotes/one/gone
  [1]
  $ git --git-dir=.josh show-ref --verify --quiet refs/josh/remotes/one/heads/gone
  [1]

The protocol adapter accepts only a configured store/projection/remote identity.

  $ josh serve-worktree --namespace missing/one upload-pack "${TESTTMP}/store/.josh" </dev/null 2>&1 | sed -n '1p'
  Error: Unknown worktree namespace 'missing/one'
  $ josh serve-worktree --namespace root/missing upload-pack "${TESTTMP}/store/.josh" </dev/null 2>&1 | sed -n '1p'
  Error: Unknown worktree namespace 'root/missing'
  $ josh serve-worktree --namespace root/one upload-pack "${TESTTMP}/one" </dev/null 2>&1 | sed -n '1s|.*one.*|Error: repository is not a josh store|p'
  Error: repository is not a josh store

A full worktree can combine disjoint mounts. Relaying to one upstream removes
the other mount, so peer content cannot leak.

  $ josh worktree add full --ref refs/remotes/one/master 2>/dev/null >/dev/null
  $ git -C full merge -q --allow-unrelated-histories -m combined two/master
  $ echo combined > full/one/app/file.txt
  $ git -C full add one/app/file.txt
  $ git -C full commit -q -m combined-change -m 'Change-Id: Irfc'
  $ cd full
  $ josh push one HEAD:refs/heads/combined --base master >/dev/null 2>&1
  $ cd ..
  $ git -C ../one show combined:app/file.txt
  combined
  $ git -C ../one cat-file -e combined:two
  fatal: path 'two' does not exist in 'combined'
  [128]

Publishing records the native revision in canonical superhistory metadata.
The independent retention ref protects it without making native history a
parent of the synthetic superhistory.

  $ cd full
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"
  $ josh changes comment Irfc -m "shared comment" >/dev/null
  $ (cd ../.josh && git-tree-pretty refs/josh/super:changes/Irfc/metadata) | grep -c message
  1
  $ josh changes publish one HEAD:refs/heads/review --base master >/dev/null 2>&1
  $ cd ..
  $ test "$(git --git-dir=.josh show refs/josh/super:changes/Irfc/projections/one/refs/heads/review/commit | wc -c | tr -d ' ')" -ge 40
  $ test "$(git --git-dir=.josh show refs/josh/super:changes/Irfc/publications/one/refs/heads/review/pushed)" = "$(git --git-dir=.josh show refs/josh/super:changes/Irfc/projections/one/refs/heads/review/commit)"
  $ (cd .josh && git-tree-pretty refs/josh/super:changes/Irfc/metadata) | grep -c message
  1
  $ git --git-dir=.josh show -s --format=%P refs/josh/super | wc -w | tr -d ' '
  1
  $ git --git-dir=.josh show -s --format=%P refs/josh/remotes/one/changes/review | wc -w | tr -d ' '
  1
  $ anchor=$(git --git-dir=.josh show -s --format=%P refs/josh/remotes/one/changes/review)
  $ git --git-dir=.josh show -s --format=%P "${anchor}" | wc -w | tr -d ' '
  1

Amending the same logical change advances both histories while the retention
first-parent chain keeps the prior native revision reachable.

  $ old_revision=$(git --git-dir=.josh show refs/josh/super:changes/Irfc/projections/one/refs/heads/review/commit)
  $ old_retention=$(git --git-dir=.josh rev-parse refs/josh/remotes/one/changes/review)
  $ echo amended > full/one/app/file.txt
  $ git -C full add one/app/file.txt
  $ git -C full commit -q --amend -m combined-change -m 'Change-Id: Irfc'
  $ cd full
  $ josh changes publish one HEAD:refs/heads/review --base master >/dev/null 2>&1
  $ cd ..
  $ new_revision=$(git --git-dir=.josh show refs/josh/super:changes/Irfc/projections/one/refs/heads/review/commit)
  $ test "${old_revision}" != "${new_revision}"
  $ git --git-dir=.josh merge-base --is-ancestor "${old_retention}" refs/josh/remotes/one/changes/review
  $ git --git-dir=.josh cat-file -e "${old_revision}^{commit}"

Removal refuses dirty worktrees, updates the registry in the superhistory, and
drops a projection only after its last worktree is gone.

  $ echo dirty > one-a/dirty.txt
  $ josh worktree remove one-a 2>&1 | sed -n '1p'
  Error: Worktree * has uncommitted changes; use --force (glob)
  $ josh worktree remove one-a --force
  $ (cd .josh && git-tree-pretty refs/josh/super:store/worktrees) | grep -c one-a/ || true
  0
  $ (cd .josh && git-tree-pretty refs/josh/super:store) | grep -c ':/one/app'
  2
  $ josh worktree remove one-b --force
  $ (cd .josh && git-tree-pretty refs/josh/super:store) | grep -c ':/one/app' || true
  0
  $ git --git-dir=.josh cat-file -e refs/josh/super:changes/Irfc/projections/one/refs/heads/review/commit
