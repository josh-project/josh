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

  $ cat .gitmodules
  [submodule "libs"]
  	path = libs
  	url = ../submodule-repo

  $ git ls-tree HEAD
  100644 blob 5255711b4fd563af2d873bf3c8f9da6c37ce1726\t.gitmodules (esc)
  160000 commit 00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd\tlibs (esc)
  100644 blob bcb9dcad21591bd9284afbb6c21e6d69eafe8f15\tmain.txt (esc)

Test UnSubmodule filter - should expand submodule into actual tree content

  $ josh-filter -s :unsubmodule master --update refs/josh/filter/master
  [2] :prefix=libs
  [3] :unsubmodule
  $ git log --graph --pretty=%s refs/josh/filter/master
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

  $ git ls-tree refs/josh/filter/master
  040000 tree e06b912df6ae0105e3a525f7a9427d98574fbc4f\tlibs (esc)
  100644 blob bcb9dcad21591bd9284afbb6c21e6d69eafe8f15\tmain.txt (esc)

  $ git ls-tree refs/josh/filter/master libs
  040000 tree e06b912df6ae0105e3a525f7a9427d98574fbc4f\tlibs (esc)

  $ git ls-tree refs/josh/filter/master libs/foo
  040000 tree 81a0b9c71d7fac4f553b2a52b9d8d52d07dd8036\tlibs/foo (esc)

  $ git ls-tree refs/josh/filter/master libs/bar
  040000 tree bd42a3e836f59dda9f9d5950d0e38431c9b1bfb5\tlibs/bar (esc)

  $ git show refs/josh/filter/master:libs/foo/file1.txt
  foo content

  $ git show refs/josh/filter/master:libs/foo/file2.txt
  bar content

  $ git show refs/josh/filter/master:libs/bar/file3.txt
  baz content

Test that .gitmodules file is removed after unsubmodule

  $ git ls-tree refs/josh/filter/master | grep gitmodules
  [1]

Test UnSubmodule with multiple submodules

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

  $ cat .gitmodules
  [submodule "libs"]
  	path = libs
  	url = ../submodule-repo
  [submodule "modules/another"]
  	path = modules/another
  	url = ../another-submodule

  $ josh-filter -s :unsubmodule master --update refs/josh/filter/master
  [1] :prefix=another
  [1] :prefix=modules
  [2] :prefix=libs
  [4] :unsubmodule
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

  $ git ls-tree refs/josh/filter/master modules
  040000 tree 3b3ee7ba855155941a68a17379e74feb261d9ab2\tmodules (esc)

  $ git show refs/josh/filter/master:modules/another/another.txt
  another content

Test UnSubmodule with submodule changes - add commits to submodule and update

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
  From /tmp/prysk-tests-6ai1pji4/unsubmodule.t/submodule-repo
     00c8fe9..47f1d80  master     -> origin/master
  Submodule path 'libs': checked out '47f1d800e93b0892d3bc525632c9ffc8d32eeb4c'
  $ git add libs
  $ git commit -m "update libs submodule" 1> /dev/null

  $ josh-filter -s :unsubmodule master --update refs/josh/filter/master
  [1] :prefix=another
  [1] :prefix=modules
  [4] :prefix=libs
  [5] :unsubmodule
  $ git log --graph --pretty=%s refs/josh/filter/master
  *   update libs submodule
  |\  
  | * add file4.txt
  | * add file3.txt
  * |   add another submodule
  |\ \  
  | * | add another.txt
  |  /  
  * | add libs submodule
  |\| 
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

  $ git show refs/josh/filter/master:libs/libs/foo/file3.txt
  new content

  $ git show refs/josh/filter/master:libs/libs/bar/file4.txt
  another new content

Test UnSubmodule on repo without submodules (should be no-op)

  $ cd ${TESTTMP}
  $ git init -q no-submodules 1> /dev/null
  $ cd no-submodules
  $ echo "content" > file.txt
  $ git add file.txt
  $ git commit -m "add file" 1> /dev/null

  $ josh-filter -s :unsubmodule master --update refs/josh/filter/master
  [1] :unsubmodule
  $ git ls-tree --name-only -r refs/josh/filter/master
  file.txt

  $ git show refs/josh/filter/master:file.txt
  content

Test UnSubmodule on repo with .gitmodules but no actual submodule entries

  $ cd ${TESTTMP}
  $ git init -q empty-submodules 1> /dev/null
  $ cd empty-submodules
  $ echo "content" > file.txt
  $ git add file.txt
  $ git commit -m "add file" 1> /dev/null

  $ cat > .gitmodules <<EOF
  $ git add .gitmodules
