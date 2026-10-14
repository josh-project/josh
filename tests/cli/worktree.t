  $ export TESTTMP=${PWD}


Create an upstream monorepo.
  $ git init -q upstream


  $ cd upstream


  $ mkdir -p sub


  $ echo root > root.txt


  $ echo one > sub/file.txt


  $ git add .


  $ git commit -q -m initial


  $ cd ${TESTTMP}


store initialization uses gitoxide and does not require a Git executable.
  $ josh_bin=$(which josh)
  $ PATH=/nonexistent "${josh_bin}" --store store init >/dev/null
  $ test -d store/.josh
  $ test ! -e store/.git
  $ git --git-dir=store/.josh config --bool core.bare
  true


  $ cd store
  $ git rev-parse --git-dir >/dev/null 2>&1; echo $?
  128


  $ josh remote add mono ${TESTTMP}/upstream 2>/dev/null

The checkout-less store HEAD follows its real synthetic history.
  $ git --git-dir=.josh symbolic-ref HEAD
  refs/josh/super
  $ git --git-dir=.josh rev-parse --verify --quiet HEAD >/dev/null


  $ (cd .josh && git-tree-pretty refs/josh/super:remotes)
  .
  └── mono/
      ├── HEAD ⇒ [commit *, tree *] (glob)
      └── refs/
          └── heads/
              └── master ⇒ [commit *, tree *] (glob)
  $ (cd .josh && git-tree-pretty refs/josh/super:store/remotes) | sed "s|file://${TESTTMP}|\${TESTTMP}|g"
  .
  └── mono/
      ├── fetch
      │   ╵  +refs/heads/*:refs/namespaces/remote-mono/refs/namespaces/mono/refs/heads/*
      \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 head (escaped)
      \xe2\x94\x82   \xe2\x95\xb5  master (escaped)
      ├── mount
      │   ╵  mono
      ├── namespace
      │   ╵  remote-mono
      └── url
          ╵  ${TESTTMP}/upstream
  $ (cd .josh && git-tree-pretty refs/josh/super:store/projections)
  .
  └── root/
      └── filter
          ╵  :/
  $ (cd .josh && git-tree-pretty refs/josh/super:store/worktrees)
  .


  $ test ! -e root.txt


  $ josh worktree add projected :/mono/sub 2>/dev/null | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added worktree at ${TESTTMP}/store/projected


  $ (cd projected && git-tree-pretty HEAD)
  .
  └── file.txt
      ┆  one


  $ git -C projected remote
  mono


  $ git -C projected status --short --branch
  ## master...mono/master


Remote mounts must be non-root relative paths and may not overlap.
  $ josh remote add / ${TESTTMP}/upstream 2>/dev/null && echo accepted || echo rejected
  rejected


  $ josh remote add :prefix=prefix ${TESTTMP}/upstream 2>/dev/null && echo accepted || echo rejected
  rejected


  $ josh remote add mono/nested ${TESTTMP}/upstream 2>/dev/null && echo accepted || echo rejected
  rejected


  $ josh remote add other ${TESTTMP}/upstream 2>/dev/null


  $ josh worktree add other-view :/other/sub 2>/dev/null >/dev/null


  $ (cd other-view && git-tree-pretty HEAD)
  .
  └── file.txt
      ┆  one


  $ josh worktree remove other-view


projection to its mounted upstream path.
  $ cd projected


  $ echo two > file.txt


  $ git add file.txt


  $ git commit -q -m projected-change


  $ git push -q mono HEAD:refs/heads/parked


  $ git -C ${TESTTMP}/upstream show-ref --verify --quiet refs/heads/parked
  [1]


  $ josh push mono HEAD:refs/heads/feature --base master >/dev/null 2>&1


  $ git -C ${TESTTMP}/upstream show feature:sub/file.txt
  two


  $ git -C ${TESTTMP}/upstream show feature:root.txt
  root


remote-tracking refs.
  $ cd ${TESTTMP}/upstream


  $ echo later > sub/later.txt


  $ git add sub/later.txt


  $ git commit -q -m later


  $ cd ${TESTTMP}/store


  $ josh fetch --remote mono 2>/dev/null


  $ (cd projected && git-tree-pretty mono/master)
  .
  ├── file.txt
  │   ┆  one
  └── later.txt
      ┆  later


  $ git --git-dir=.josh rev-list --count refs/josh/super
  10


  $ git --git-dir=.josh show -s --format=%P refs/josh/super | wc -w | tr -d ' '
  1


Worktree registry operations are store-wide.
  $ josh worktree list | sed "s|${TESTTMP}|\${TESTTMP}|g"
  ${TESTTMP}/store/projected	*	:/mono/sub (glob)


  $ josh worktree remove projected


  $ test ! -e projected


local branch tip in the linked worktree.
  $ cd ${TESTTMP}


  $ git clone -q upstream legacy


  $ mkdir -p legacy/.git/josh/remotes


  $ printf '%s\n' ':~(' "    fetch=\"+refs/heads/*:refs/josh/remotes/origin/*\"" "    url=\"${TESTTMP}/upstream\"" ')[' '    :/' ']' > legacy/.git/josh/remotes/origin.josh


  $ echo local > legacy/local.txt


  $ git -C legacy add local.txt


  $ git -C legacy commit -q -m local


  $ josh migrate legacy 2>/dev/null | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Migrated worktree to ${TESTTMP}/legacy/worktree
  $ test -d legacy/.josh
  $ test ! -e legacy/.git
  $ git --git-dir=legacy/.josh config --bool core.bare
  true

  $ git --git-dir=legacy/.josh symbolic-ref HEAD
  refs/josh/super
  $ git --git-dir=legacy/.josh rev-parse --verify --quiet HEAD >/dev/null


  $ test ! -e legacy/root.txt


  $ git -C legacy/worktree show HEAD:local.txt
  local
