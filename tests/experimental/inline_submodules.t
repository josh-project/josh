  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q submodule-repo 1> /dev/null
  $ cd submodule-repo

  $ mkdir -p foo
  $ echo "foo content" > foo/file1.txt
  $ echo "bar content" > foo/file2.txt
  $ git add foo
  $ git commit -m "add foo with files" 1> /dev/null

  $ mkdir -p bar
  $ echo "baz content" > bar/file3.txt
  $ git add bar
  $ git commit -m "add bar with file" 1> /dev/null

  $ cd ${TESTTMP}
  $ git init -q main-repo 1> /dev/null
  $ cd main-repo
  $ git commit -m "init" --allow-empty 1> /dev/null

  $ echo "main content" > main.txt
  $ git add main.txt
  $ git commit -m "add main.txt" 1> /dev/null

  $ git submodule add ../submodule-repo libs 2> /dev/null
  $ git submodule status
   00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd libs (heads/master)

  $ git commit -m "add libs submodule" 1> /dev/null

  $ git fetch ../submodule-repo
  From ../submodule-repo
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s
  * add libs submodule
  * add main.txt
  * init

  $ git ls-tree --name-only -r HEAD
  .gitmodules
  libs
  main.txt

Test InlineSubmodules filter - should inline submodule tree content and add the
submodule history as an extra parent at each pointer update.

  $ josh-filter -s :inline_submodules master --update refs/josh/filter/master
  2ffcff2eab987cbae3d21c9ffca4741298ca7338
  [1] :/libs
  [3] :inline_submodules
  [3] reachable_roots
  [3] sequence_number

  $ git log --graph --pretty=%s refs/josh/filter/master
  *   add libs submodule
  |\  
  | * add bar with file
  | * add foo with files
  * add main.txt
  * init

The filtered tree should have the submodule files inlined at libs/ and no
.gitmodules:

  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/bar/file3.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  main.txt

  $ git show refs/josh/filter/master:libs/foo/file1.txt
  foo content

  $ git show refs/josh/filter/master:libs/foo/file2.txt
  bar content

  $ git show refs/josh/filter/master:libs/bar/file3.txt
  baz content

Test .gitmodules is absent

  $ git ls-tree refs/josh/filter/master | grep gitmodules
  [1]

Test InlineSubmodules with multiple submodules

  $ cd ${TESTTMP}
  $ git init -q another-submodule 1> /dev/null
  $ cd another-submodule
  $ echo "another content" > another.txt
  $ git add another.txt
  $ git commit -m "add another.txt" 1> /dev/null

  $ cd ${TESTTMP}/main-repo
  $ git submodule add ../another-submodule modules/another 2> /dev/null
  $ git commit -m "add another submodule" 1> /dev/null

  $ git fetch ../another-submodule
  From ../another-submodule
   * branch            HEAD       -> FETCH_HEAD

  $ josh-filter -s :inline_submodules master --update refs/josh/filter/master
  49287de5bf4b68fb60ef2df8d2676c25a78edcbc
  [1] :/another
  [1] :/libs
  [3] :/modules
  [4] :inline_submodules
  [7] reachable_roots
  [7] sequence_number

  $ git log --graph --pretty=%s refs/josh/filter/master
  *   add another submodule
  |\  
  | * add another.txt
  *   add libs submodule
  |\  
  | * add bar with file
  | * add foo with files
  * add main.txt
  * init

  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/bar/file3.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  main.txt
  modules/another/another.txt

  $ git show refs/josh/filter/master:modules/another/another.txt
  another content

Test InlineSubmodules with submodule update - moving the libs pointer should
become a merge with the additional submodule commits as the second parent.

  $ cd ${TESTTMP}/submodule-repo
  $ mkdir -p libs/foo libs/bar
  $ echo "new content" > libs/foo/file3.txt
  $ git add libs/foo/file3.txt
  $ git commit -m "add file3.txt" 1> /dev/null

  $ echo "another new content" > libs/bar/file4.txt
  $ git add libs/bar/file4.txt
  $ git commit -m "add file4.txt" 1> /dev/null

  $ cd ${TESTTMP}/main-repo
  $ git fetch ../submodule-repo
  From ../submodule-repo
   * branch            HEAD       -> FETCH_HEAD
  $ git submodule update --remote libs
  From /tmp/prysk-tests-*/inline_submodules.t/submodule-repo (glob)
     00c8fe9..47f1d80  master     -> origin/master
  Submodule path 'libs': checked out '47f1d800e93b0892d3bc525632c9ffc8d32eeb4c'
  $ git add libs
  $ git commit -m "update libs submodule" 1> /dev/null

  $ josh-filter -s :inline_submodules master --update refs/josh/filter/master
  3af469419c26b5fd7d3d708a23e9273f55ac1355
  [1] :/another
  [3] :/modules
  [5] :inline_submodules
  [6] :/libs
  [12] reachable_roots
  [12] sequence_number

  $ git log --graph --pretty=%s refs/josh/filter/master
  *   update libs submodule
  |\  
  | * add file4.txt
  | * add file3.txt
  |/  
  *   add another submodule
  |\  
  | * add another.txt
  *   add libs submodule
  |\  
  | * add bar with file
  | * add foo with files
  * add main.txt
  * init

  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/bar/file3.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  libs/libs/bar/file4.txt
  libs/libs/foo/file3.txt
  main.txt
  modules/another/another.txt

  $ git show refs/josh/filter/master:libs/foo/file3.txt
  fatal: path 'libs/foo/file3.txt' does not exist in 'refs/josh/filter/master'
  [128]

  $ git show refs/josh/filter/master:libs/bar/file4.txt
  fatal: path 'libs/bar/file4.txt' does not exist in 'refs/josh/filter/master'
  [128]

Test InlineSubmodules on repo without submodules (should be no-op)

  $ cd ${TESTTMP}
  $ git init -q no-submodules 1> /dev/null
  $ cd no-submodules
  $ echo "content" > file.txt
  $ git add file.txt
  $ git commit -m "add file" 1> /dev/null

  $ josh-filter -s :inline_submodules master --update refs/josh/filter/master
  62f270988ac3ab05a33d0705fb1e4982165f69f2
  [1] :inline_submodules
  [1] reachable_roots
  [1] sequence_number

  $ git ls-tree --name-only -r refs/josh/filter/master
  file.txt

  $ git show refs/josh/filter/master:file.txt
  content
