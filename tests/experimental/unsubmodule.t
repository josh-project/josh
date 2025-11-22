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

Test Adapt filter - should expand submodule into actual tree content

  $ josh-filter -s :adapt=submodules master --update refs/josh/filter/master
  [3] :adapt=submodules
  [3] sequence_number
  $ git log --graph --pretty=%s refs/josh/filter/master
  * add libs submodule
  * add main.txt
  * init
  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/.josh-link.toml
  main.txt

  $ git ls-tree refs/josh/filter/master
  040000 tree 34bc8209dca31283563d5519e297ae8cc7f0f19a\tlibs (esc)
  100644 blob bcb9dcad21591bd9284afbb6c21e6d69eafe8f15\tmain.txt (esc)

  $ git ls-tree refs/josh/filter/master libs
  040000 tree 34bc8209dca31283563d5519e297ae8cc7f0f19a\tlibs (esc)

  $ git ls-tree refs/josh/filter/master libs/foo

  $ git ls-tree refs/josh/filter/master libs/bar

  $ git show refs/josh/filter/master:libs/foo/file1.txt
  fatal: path 'libs/foo/file1.txt' exists on disk, but not in 'refs/josh/filter/master'
  [128]

  $ git show refs/josh/filter/master:libs/foo/file2.txt
  fatal: path 'libs/foo/file2.txt' exists on disk, but not in 'refs/josh/filter/master'
  [128]

  $ git show refs/josh/filter/master:libs/bar/file3.txt
  fatal: path 'libs/bar/file3.txt' exists on disk, but not in 'refs/josh/filter/master'
  [128]

Test that .gitmodules file is removed after unsubmodule

  $ git ls-tree refs/josh/filter/master | grep gitmodules
  [1]

Test Adapt with multiple submodules

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

  $ josh-filter -s :adapt=submodules master --update refs/josh/filter/master
  [4] :adapt=submodules
  [4] sequence_number
  $ git log --graph --pretty=%s refs/josh/filter/master
  * add another submodule
  * add libs submodule
  * add main.txt
  * init
  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/.josh-link.toml
  main.txt
  modules/another/.josh-link.toml

  $ git ls-tree refs/josh/filter/master modules
  040000 tree 9dd65d88b3c43c244c71187f86c40f77e771e432\tmodules (esc)

  $ git show refs/josh/filter/master:modules/another/another.txt
  fatal: path 'modules/another/another.txt' exists on disk, but not in 'refs/josh/filter/master'
  [128]

Test Adapt with submodule changes - add commits to submodule and update

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
  From /tmp/prysk-tests-*/unsubmodule.t/submodule-repo (glob)
     00c8fe9..47f1d80  master     -> origin/master
  Submodule path 'libs': checked out '47f1d800e93b0892d3bc525632c9ffc8d32eeb4c'
  $ git add libs
  $ git commit -m "update libs submodule" 1> /dev/null

  $ josh-filter -s :adapt=submodules master --update refs/josh/filter/master
  [5] :adapt=submodules
  [5] sequence_number
  $ git log --graph --pretty=%s refs/josh/filter/master
  * update libs submodule
  * add another submodule
  * add libs submodule
  * add main.txt
  * init
  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/.josh-link.toml
  main.txt
  modules/another/.josh-link.toml

  $ git show refs/josh/filter/master:libs/libs/foo/file3.txt
  fatal: path 'libs/libs/foo/file3.txt' exists on disk, but not in 'refs/josh/filter/master'
  [128]

  $ git show refs/josh/filter/master:libs/libs/bar/file4.txt
  fatal: path 'libs/libs/bar/file4.txt' exists on disk, but not in 'refs/josh/filter/master'
  [128]

Test Adapt on repo without submodules (should be no-op)

  $ cd ${TESTTMP}
  $ git init -q no-submodules 1> /dev/null
  $ cd no-submodules
  $ echo "content" > file.txt
  $ git add file.txt
  $ git commit -m "add file" 1> /dev/null

  $ josh-filter -s :adapt=submodules master --update refs/josh/filter/master
  [1] :adapt=submodules
  [1] sequence_number
  $ git ls-tree --name-only -r refs/josh/filter/master
  file.txt

  $ git show refs/josh/filter/master:file.txt
  content

Test Adapt on repo with .gitmodules but no actual submodule entries

  $ cd ${TESTTMP}
  $ git init -q empty-submodules 1> /dev/null
  $ cd empty-submodules
  $ echo "content" > file.txt
  $ git add file.txt
  $ git commit -m "add file" 1> /dev/null

  $ cat > .gitmodules <<EOF
  $ git add .gitmodules
