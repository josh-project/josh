  $ export TESTTMP=${PWD}

  $ git init -q upstream
  $ echo contents > upstream/file
  $ git -C upstream add file
  $ git -C upstream commit -q -m initial
  $ git clone -q upstream legacy
  $ git -C legacy config josh-remote.origin.url "file://${TESTTMP}/upstream"
  $ git -C legacy config josh-remote.origin.filter ":/"
  $ git -C legacy config josh-remote.origin.fetch "+refs/heads/*:refs/josh/remotes/origin/*"

Legacy Git-config remotes are accepted by the store migration helper.

  $ cd legacy
  $ josh migrate 2>/dev/null | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Migrated worktree to ${TESTTMP}/legacy/worktree
  $ git -C worktree show HEAD:file
  contents
  $ test -f .josh/josh/remotes/origin.josh
