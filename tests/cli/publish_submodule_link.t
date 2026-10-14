  $ export TESTTMP=${PWD}
  $ export JOSH_EXPERIMENTAL_FEATURES=1
  $ git config --global protocol.file.allow always

Create a submodule remote and a superproject that points at it. The submodule
commit is deliberately absent from the superproject object database.

  $ git init -q lib
  $ cd lib
  $ echo "lib v1" > lib.txt
  $ git add lib.txt
  $ git commit -q -m "lib initial"
  $ LIB_BASE=$(git rev-parse HEAD)
  $ cd ${TESTTMP}

  $ git init -q --bare super-remote
  $ git init -q super
  $ cd super
  $ git remote add origin ../super-remote
  $ echo "main" > main.txt
  $ git add main.txt
  $ git commit -q -m "main initial"
  $ git submodule add -q --name lib ../lib modules/lib
  $ git commit -q -m "add lib submodule"
  $ git push -q origin master
  $ git cat-file -e ${LIB_BASE}^{commit} 2>/dev/null
  [128]

Remote discovery adds the superproject and its configured submodule as separate
store remotes. Inlining and export-link creation happen only when the combined
worktree is created.

  $ cd ${TESTTMP}
  $ josh --store store init >/dev/null
  $ cd store
  $ josh remote add super ../super-remote --submodules --no-forge 2>/dev/null
  $ (cd .josh && git-tree-pretty refs/josh/super:remotes)
  .
  ├── lib/
  │   ├── HEAD ⇒ [commit *, tree *] (glob)
  │   └── refs/
  │       └── heads/
  │           └── master ⇒ [commit *, tree *] (glob)
  └── super/
      ├── HEAD ⇒ [commit *, tree *] (glob)
      └── refs/
          └── heads/
              └── master ⇒ [commit *, tree *] (glob)
  $ josh worktree add combined --submodules --remote super 2>/dev/null | sed "s|file://${TESTTMP}|\${TESTTMP}|g; s|${TESTTMP}|\${TESTTMP}|g"
  Added submodule link 'lib': ${TESTTMP}/lib refs/heads/master :/modules/lib:export
  Added worktree at ${TESTTMP}/store/combined
  $ cd combined
  $ git remote
  lib
  super
  $ git cat-file -t ${LIB_BASE}
  commit
  $ test "$(git rev-parse refs/josh/submodules/lib)" = "${LIB_BASE}" && echo pinned
  pinned
  $ josh link list | sed "s|file://${TESTTMP}|\${TESTTMP}|g"
  lib\t${TESTTMP}/lib\trefs/heads/master\t\t:/modules/lib:export (escaped)
  $ git show -s --format=%s HEAD^2
  lib initial

A change made in the combined history publishes to the submodule link. Its
change ref must descend directly from the original submodule tip, not from the
superproject history.

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"
  $ echo "lib v2" > modules/lib/lib.txt
  $ git add modules/lib/lib.txt
  $ git commit -q -m "lib v2" -m "Change-Id: lib2"
  $ josh changes publish super 2>&1 | sed "s|file://${TESTTMP}|\${TESTTMP}|g"
  published 1 change (1 new)
  Publishing to link 'lib' (${TESTTMP}/lib):
  published 1 change (1 new)

  $ test "$(git -C ${TESTTMP}/lib rev-parse refs/heads/@changes/master/josh@example.com/lib2^)" = "${LIB_BASE}" && echo based-on-pinned
  based-on-pinned
  $ git -C ${TESTTMP}/lib show refs/heads/@changes/master/josh@example.com/lib2:lib.txt
  lib v2
