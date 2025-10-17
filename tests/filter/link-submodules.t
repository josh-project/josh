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

  $ git log --graph --pretty=%s:%H
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738

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

  $ josh-filter -s :adapt=submodules:link master --update refs/josh/filter/master
  [2] :prefix=libs
  [3] :adapt=submodules
  [3] :link=embedded
  $ git log --graph --pretty=%s refs/josh/filter/master
  *   add libs submodule
  |\  
  | * add bar with file
  | * add foo with files
  * add main.txt
  * init
  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/.josh-link.toml
  libs/bar/file3.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  main.txt

  $ git ls-tree refs/josh/filter/master
  040000 tree 1a06220380a0dd3249b08cb1b69158338ebad3ef\tlibs (esc)
  100644 blob bcb9dcad21591bd9284afbb6c21e6d69eafe8f15\tmain.txt (esc)

  $ git ls-tree refs/josh/filter/master libs
  040000 tree 1a06220380a0dd3249b08cb1b69158338ebad3ef\tlibs (esc)

  $ git ls-tree refs/josh/filter/master libs/foo
  040000 tree 81a0b9c71d7fac4f553b2a52b9d8d52d07dd8036\tlibs/foo (esc)

  $ git ls-tree refs/josh/filter/master libs/bar
  040000 tree bd42a3e836f59dda9f9d5950d0e38431c9b1bfb5\tlibs/bar (esc)

  $ git show refs/josh/filter/master:libs/.josh-link.toml
  remote = "../submodule-repo"
  branch = "HEAD"
  filter = ":/"
  commit = "00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd"
  $ git show refs/josh/filter/master:libs/foo/file1.txt
  foo content

  $ git show refs/josh/filter/master:libs/foo/file2.txt
  bar content

  $ git show refs/josh/filter/master:libs/bar/file3.txt
  baz content

Test that .gitmodules file is removed after unsubmodule:link

  $ git ls-tree refs/josh/filter/master | grep gitmodules
  [1]

Test Adapt with multiple submodules

  $ cd ${TESTTMP}
  $ git init -q another-submodule 1> /dev/null
  $ cd another-submodule
  $ echo "another content" > another.txt
  $ git add another.txt
  $ git commit -m "add another.txt" 1> /dev/null
  $ git log --graph --pretty=%s:%H
  * add another.txt:8fbd01fa31551a059e280f68ac37397712feb59e

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
  [2] :prefix=libs
  [3] :link=embedded
  [4] :adapt=submodules
  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/.josh-link.toml
  main.txt
  modules/another/.josh-link.toml
  $ git show refs/josh/filter/master:libs/.josh-link.toml
  remote = "../submodule-repo"
  branch = "HEAD"
  filter = ":/"
  commit = "00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd"
  $ josh-filter -s :adapt=submodules:link master --update refs/josh/filter/master
  [1] :prefix=another
  [1] :prefix=modules
  [2] :prefix=libs
  [4] :adapt=submodules
  [4] :link=embedded
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
  libs/.josh-link.toml
  libs/bar/file3.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  main.txt
  modules/another/.josh-link.toml
  modules/another/another.txt

  $ git ls-tree refs/josh/filter/master modules
  040000 tree e1b10636436c25c78dc6372eb20079454f05d746\tmodules (esc)

  $ git show refs/josh/filter/master:modules/another/another.txt
  another content

Test Adapt with submodule changes - add commits to submodule and update

  $ cd ${TESTTMP}/submodule-repo
  $ echo "new content" > foo/file3.txt
  $ git add foo/file3.txt
  $ git commit -m "add file3.txt" 1> /dev/null

  $ echo "another new content" > bar/file4.txt
  $ git add bar/file4.txt
  $ git commit -m "add file4.txt" 1> /dev/null
  $ git log --graph --pretty=%s:%H
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738

  $ cd ${TESTTMP}/main-repo
  $ git fetch ../submodule-repo
  From ../submodule-repo
   * branch            HEAD       -> FETCH_HEAD
  $ git submodule update --remote libs
  From /tmp/prysk-tests-qqjv_m_8/link-submodules.t/submodule-repo
     00c8fe9..3061af9  master     -> origin/master
  Submodule path 'libs': checked out '3061af908a0dc1417902fbd7208bb2b8dc354e6c'
  $ git add libs
  $ git commit -m "update libs submodule" 1> /dev/null

  $ tree
  .
  |-- libs
  |   |-- bar
  |   |   |-- file3.txt
  |   |   `-- file4.txt
  |   `-- foo
  |       |-- file1.txt
  |       |-- file2.txt
  |       `-- file3.txt
  |-- main.txt
  `-- modules
      `-- another
          `-- another.txt
  
  6 directories, 7 files


  $ josh-filter -s :adapt=submodules:link master --update refs/josh/filter/master
  [1] :prefix=another
  [1] :prefix=modules
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :link=embedded
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
  libs/.josh-link.toml
  libs/bar/file3.txt
  libs/bar/file4.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  libs/foo/file3.txt
  main.txt
  modules/another/.josh-link.toml
  modules/another/another.txt

  $ git show refs/josh/filter/master:libs/foo/file3.txt
  new content

  $ git show refs/josh/filter/master:libs/bar/file4.txt
  another new content

  $ josh-filter -s :adapt=submodules:link:/libs master
  [1] :prefix=another
  [1] :prefix=modules
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :link=embedded
  [9] :/libs
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:93f3162c2e8d78320091bb8bb7f9b27226f105bc
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:cb64d3e5db01b0b451f21199ae2197997bc592ba
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link:/libs:prune=trivial-merge master
  [1] :prefix=another
  [1] :prefix=modules
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:93f3162c2e8d78320091bb8bb7f9b27226f105bc
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:cb64d3e5db01b0b451f21199ae2197997bc592ba
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -p --reverse :prune=trivial-merge:export:prefix=libs
  :/libs:export:prune=trivial-merge
  $ josh-filter -s :adapt=submodules:link:/libs:export master
  [1] :prefix=another
  [1] :prefix=modules
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  $ git log --graph --pretty=%s:%H:%T FILTERED_HEAD
  *   update libs submodule:6b4dc1befca9cf2115f162241027b371e4313b54:ac420a625dfb874002210e623a7fdb55708ef2fa
  |\  
  * | add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c:ac420a625dfb874002210e623a7fdb55708ef2fa
  * | add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173:2935d839ce5e2fa8d5d8fb1a8541bf95b98fbedb
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd:e06b912df6ae0105e3a525f7a9427d98574fbc4f
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738:7ca6af9b9a7d0d7f4723a74cf6006c14eaea547e
  $ josh-filter -s :adapt=submodules:link:/libs:export:prune=trivial-merge master
  [1] :prefix=another
  [1] :prefix=modules
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link:/modules/another master
  [1] :prefix=another
  [1] :prefix=modules
  [2] :/another
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :/modules
  [6] :prune=trivial-merge
  [9] :/libs
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * add another submodule:88584f5d636e6478f0ddec62e6b665625c7a3350
  * add another.txt:8fbd01fa31551a059e280f68ac37397712feb59e

  $ josh-filter -s :adapt=submodules:link master --update refs/heads/testsubexport
  [1] :prefix=another
  [1] :prefix=modules
  [2] :/another
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :/modules
  [6] :prune=trivial-merge
  [9] :/libs
  $ rm -Rf libs
  $ rm -Rf modules
  $ git checkout testsubexport
  Switched to branch 'testsubexport'
  $ echo "fo" > libs/foobar.txt
  $ git add .
  $ git commit -m "mod libs submodule" 1> /dev/null
  $ git log --graph --pretty=%s:%H
  * mod libs submodule:7f9d8df9e88d6f27ee110fc67d01ba1b87e42571
  *   update libs submodule:b4a38ac7eeb1be6dca8cd0008248ae3ffcb5e980
  |\  
  | * add file4.txt:709f22fcdf590b5057efab18017bfbbd7be7079c
  | * add file3.txt:114bdaee845b512f66ce1a93a35bbd4ad8b11eba
  * |   add another submodule:61d3ca04cec95199f9dc686c2197b171cdf6ccc0
  |\ \  
  | * | add another.txt:7783d79da01d27682016e2ed55b36844e02046f7
  |  /  
  * | add libs submodule:11c413afc6f34cc0e926034e00de843729ac4853
  |\| 
  | * add bar with file:2a76239d0444a879122b9327266190a20f4fc485
  | * add foo with files:5a9cfbaaa4d0d3b4df88374c16591cda53149817
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ josh-filter -s ":/libs:export:prune=trivial-merge" --update refs/heads/testsubexported
  [1] :prefix=another
  [1] :prefix=modules
  [2] :/another
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :/modules
  [6] :export
  [7] :prune=trivial-merge
  [10] :/libs

  $ git checkout testsubexported
  Switched to branch 'testsubexported'
  $ git log --graph --pretty=%s:%H
  * mod libs submodule:005cde5c84fbcf17526a0e2fec0a2932c4ce8f24
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738

  $ git rebase 3061af908a0dc1417902fbd7208bb2b8dc354e6c
  Current branch testsubexported is up to date.
  $ git log --graph --pretty=%s:%H
  * mod libs submodule:005cde5c84fbcf17526a0e2fec0a2932c4ce8f24
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738

  $ josh-filter -s ":adapt=submodules:link:unlink" refs/heads/master --update refs/heads/unlinked_master
  [1] :prefix=another
  [1] :prefix=modules
  [1] :prefix=modules/another
  [2] :/another
  [4] :prefix=libs
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :/modules
  [6] :export
  [7] :prune=trivial-merge
  [10] :/libs
  [10] :unlink

  $ git log --graph --pretty=%s:%H:%T refs/heads/unlinked_master
  * update libs submodule:407617e7655c7e0755d62908c87a5a9753d02f57:03b3f655c4ce3f3d8a6fba82bba301c59cc1d957
  * add another submodule:12795780a32b67904ba9f3a4c4675bb0ef28f64c:956f44c3fddbdc526cbf74825ae07c83bde636fd
  * add libs submodule:37be6f165058a59c87dfdfad4c56489d50251aea:a860693798b958c292bddba8b9f9c64f5b1f8680
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd:1eedb83532c1049f67f2d851fe666e23dee45a6f
  * init:01d3837a9f7183df88e956cc81f085544f9c6563:4b825dc642cb6eb9a060e54bf8d69288fbee4904


Test Adapt on repo without submodules (should be no-op)

  $ cd ${TESTTMP}
  $ git init -q no-submodules 1> /dev/null
  $ cd no-submodules
  $ echo "content" > file.txt
  $ git add file.txt
  $ git commit -m "add file" 1> /dev/null

  $ josh-filter -s :adapt=submodules:link master --update refs/josh/filter/master
  [1] :adapt=submodules
  [1] :link=embedded
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
