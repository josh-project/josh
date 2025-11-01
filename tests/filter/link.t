Test Link filter (identical to Adapt)

  $ export TESTTMP=${PWD}
  $ cd ${TESTTMP}
  $ git init -q main-repo 1> /dev/null
  $ cd main-repo
  $ git config protocol.file.allow always
  $ echo "main content" > main.txt
  $ git add main.txt
  $ git commit -m "add main.txt" 1> /dev/null

  $ cd ${TESTTMP}
  $ git init -q submodule-repo 1> /dev/null
  $ cd submodule-repo
  $ git config protocol.file.allow always
  $ mkdir -p foo bar
  $ echo "foo content" > foo/file1.txt
  $ echo "bar content" > bar/file2.txt
  $ git add .
  $ git commit -m "add libs" 1> /dev/null

  $ cd ${TESTTMP}/main-repo
  $ git submodule add ../submodule-repo libs
  Cloning into '/tmp/prysk-tests-qqjv_m_8/link.t/main-repo/libs'...
  done.
  $ git add .gitmodules libs
  $ git commit -m "add libs submodule" 1> /dev/null

  $ git fetch ../submodule-repo
  From ../submodule-repo
   * branch            HEAD       -> FETCH_HEAD

  $ josh-filter -s :adapt=submodules:link master --update refs/josh/filter/master
  [1] :prefix=libs
  [2] :adapt=submodules
  [2] :link=embedded
  $ git ls-tree -r --name-only refs/josh/filter/master
  libs/.josh-link.toml
  libs/bar/file2.txt
  libs/foo/file1.txt
  main.txt

  $ git show refs/josh/filter/master:libs/foo/file1.txt
  foo content

  $ git show refs/josh/filter/master:libs/bar/file2.txt
  bar content

Test Link on repo without submodules (should be no-op)

  $ cd ${TESTTMP}
  $ git init -q no-submodules 1> /dev/null
  $ cd no-submodules
  $ echo "content" > file.txt
  $ git add file.txt
  $ git commit -m "add file" 1> /dev/null

  $ josh-filter -s :link master --update refs/josh/filter/master
  [1] :link=embedded
  $ git ls-tree -r --name-only refs/josh/filter/master
  file.txt
