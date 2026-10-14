  $ export TESTTMP=${PWD}


  $ git init -q upstream


  $ mkdir upstream/sub


  $ echo contents > upstream/sub/file


  $ git -C upstream add sub


  $ git -C upstream commit -q -m initial


receive ordinary remote-tracking refs.
  $ josh --store store init >/dev/null


  $ cd store


  $ josh remote add upstream ${TESTTMP}/upstream 2>/dev/null


  $ git --git-dir=.josh show refs/josh/super:store/remotes/upstream/fetch
  +refs/heads/*:refs/namespaces/remote-upstream/refs/namespaces/upstream/refs/heads/* (no-eol)


  $ josh worktree add ../projected :/upstream/sub 2>/dev/null >/dev/null


  $ git -C ../projected config --get-all remote.upstream.fetch
  +refs/heads/*:refs/remotes/upstream/*
  $ git -C ../projected config remote.upstream.uploadpack | sed 's/--namespace [^ ]*/--namespace NAMESPACE/'
  josh serve-worktree --namespace NAMESPACE upload-pack
  $ git -C ../projected config remote.upstream.receivepack | sed 's/--namespace [^ ]*/--namespace NAMESPACE/'
  josh serve-worktree --namespace NAMESPACE receive-pack


  $ git -C ../projected symbolic-ref refs/remotes/upstream/HEAD
  refs/remotes/upstream/master
