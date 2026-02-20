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
  Cloning into '/tmp/prysk-tests-*/link.t/main-repo/libs'... (glob)
  done.
  $ git add .gitmodules libs
  $ git commit -m "add libs submodule" 1> /dev/null

  $ git fetch ../submodule-repo
  From ../submodule-repo
   * branch            HEAD       -> FETCH_HEAD

  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/josh/filter/master
  27814d162ba765274145a42ae41d327137422c1b
  [1] :embed=libs
  [1] :unapply(784847e21478f8b81b2fbe8c92e20159a59773a8:/libs)
  [2] :"{@}"
  [2] ::libs/.link.josh
  [2] :adapt=submodules
  [2] :link=embedded
  [7] sequence_number
  $ git ls-tree -r --name-only refs/josh/filter/master
  libs/.link.josh
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
  62f270988ac3ab05a33d0705fb1e4982165f69f2
  [1] :link
  [1] sequence_number
  $ git ls-tree -r --name-only refs/josh/filter/master
  file.txt
