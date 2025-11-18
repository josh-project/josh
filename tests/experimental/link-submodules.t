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
  [1] :embed=libs
  [2] ::libs/.josh-link.toml
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] :"{@}"
  [3] :adapt=submodules
  [3] :link=embedded
  [10] sequence_number
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
  [1] :embed=libs
  [2] ::libs/.josh-link.toml
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] :"{@}"
  [3] :link=embedded
  [4] :adapt=submodules
  [11] sequence_number
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
  [1] :embed=libs
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::libs/.josh-link.toml
  [2] ::modules/another/.josh-link.toml
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [4] :"{@}"
  [4] :adapt=submodules
  [4] :link=embedded
  [15] sequence_number
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
  From /tmp/prysk-tests-*/link-submodules.t/submodule-repo (glob)
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
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [21] sequence_number
  $ git log --graph --pretty=%s:%H refs/josh/filter/master
  *   update libs submodule:4657e0d71754bdd097c6453a75a5084b467baf54
  |\  
  | * add file4.txt:2278f620291df7133299176efb6210fc193c3387
  | * add file3.txt:deed71e37198bf3c8668fa353c66d79c0de25834
  * |   add another submodule:b0153ca36fe220c8e0942ee5daf51512907108ca
  |\ \  
  | * | add another.txt:1f2b84bfd4029e70d3aeb16a6ecb7f0a0490490e
  |  /  
  * | add libs submodule:d7b5b1dad9444f25b5011d9f25af2e48a82ff173
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ git ls-tree --name-only -r 4657e0d71754bdd097c6453a75a5084b467baf54 
  libs/.josh-link.toml
  libs/bar/file3.txt
  libs/bar/file4.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  libs/foo/file3.txt
  main.txt
  modules/another/.josh-link.toml
  modules/another/another.txt
  $ git ls-tree --name-only -r 2278f620291df7133299176efb6210fc193c3387
  libs/bar/file3.txt
  libs/bar/file4.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  libs/foo/file3.txt
  main.txt
  modules/another/.josh-link.toml
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
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [9] :/libs
  [29] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:6336c45ef94ccdc32fd072b5d7fecf0e9755431a
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * |   add another submodule:f6d97d3185819dce5596623f5494208fca2de85d
  |\ \  
  | * | add another.txt:529c9c80186129065a994cbf91095ab1e90323f0
  |  /  
  * / add libs submodule:cb64d3e5db01b0b451f21199ae2197997bc592ba
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link:/libs:prune=trivial-merge master
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [7] :prune=trivial-merge
  [9] :/libs
  [33] sequence_number
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
  $ josh-filter -s :adapt=submodules:link:/libs:export:prune=trivial-merge master
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [7] :export
  [7] :prune=trivial-merge
  [9] :/libs
  [34] sequence_number
  $ git log --graph --pretty=%s:%H:%T FILTERED_HEAD
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c:ac420a625dfb874002210e623a7fdb55708ef2fa
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173:2935d839ce5e2fa8d5d8fb1a8541bf95b98fbedb
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd:e06b912df6ae0105e3a525f7a9427d98574fbc4f
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738:7ca6af9b9a7d0d7f4723a74cf6006c14eaea547e
  $ josh-filter -s :adapt=submodules:link:/libs:export:prune=trivial-merge master
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [7] :export
  [7] :prune=trivial-merge
  [9] :/libs
  [34] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link:/modules/another master
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :/another
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [7] :/modules
  [7] :export
  [7] :prune=trivial-merge
  [9] :/libs
  [38] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:91175b1309708c7a2ce159f274da8b9a011310ce
  |\  
  | * add file3.txt:ba5a5509adebeb19574c8abb6d4194d1744ef3f4
  * add another submodule:88584f5d636e6478f0ddec62e6b665625c7a3350
  * add another.txt:8fbd01fa31551a059e280f68ac37397712feb59e

  $ josh-filter -s :adapt=submodules:link master --update refs/heads/testsubexport
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :/another
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [7] :/modules
  [7] :export
  [7] :prune=trivial-merge
  [9] :/libs
  [38] sequence_number
  $ rm -Rf libs
  $ rm -Rf modules
  $ git checkout testsubexport
  Switched to branch 'testsubexport'
  $ echo "fo" > libs/foobar.txt
  $ git add .
  $ git commit -m "mod libs submodule" 1> /dev/null
  $ git log --graph --pretty=%s:%H
  * mod libs submodule:5a0e5c34f8199e745ba699b4c0423756b18fb1a0
  *   update libs submodule:4657e0d71754bdd097c6453a75a5084b467baf54
  |\  
  | * add file4.txt:2278f620291df7133299176efb6210fc193c3387
  | * add file3.txt:deed71e37198bf3c8668fa353c66d79c0de25834
  * |   add another submodule:b0153ca36fe220c8e0942ee5daf51512907108ca
  |\ \  
  | * | add another.txt:1f2b84bfd4029e70d3aeb16a6ecb7f0a0490490e
  |  /  
  * | add libs submodule:d7b5b1dad9444f25b5011d9f25af2e48a82ff173
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ josh-filter -s ":/libs:export:prune=trivial-merge" --update refs/heads/testsubexported
  [1] :embed=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :/another
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [7] :/modules
  [8] :export
  [8] :prune=trivial-merge
  [10] :/libs
  [41] sequence_number

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
  [1] :embed=modules/another
  [1] :prefix=modules/another
  [1] :unapply(daa965c7c3a3f8289819a728d6c0f31f0590dc6c:/modules/another)
  [2] ::modules/another/.josh-link.toml
  [2] :embed=libs
  [2] :unapply(06d10a853b133ffc533e8ec3f2ed4ec43b64670c:/libs)
  [3] ::libs/.josh-link.toml
  [4] :/another
  [4] :prefix=libs
  [4] :unapply(f4bfdb82ca5e0f06f941f68be2a0fd19573bc415:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [7] :/modules
  [8] :export
  [8] :prune=trivial-merge
  [10] :/libs
  [10] :unlink
  [41] sequence_number

  $ git log --graph --pretty=%s:%H:%T refs/heads/unlinked_master
  *   update libs submodule:41ccc704fcbdd815b6849b9927f584cf4c9f6f0e:03b3f655c4ce3f3d8a6fba82bba301c59cc1d957
  |\  
  | * add file4.txt:2278f620291df7133299176efb6210fc193c3387:10ff89ee90260a4398c847e6c9448ee6a9f8e4c7
  | * add file3.txt:deed71e37198bf3c8668fa353c66d79c0de25834:9122c83968648c7219e0fee04263e0fce0e45c55
  * |   add another submodule:67f677db1f181d00fd3f82baf39b095b73c74634:956f44c3fddbdc526cbf74825ae07c83bde636fd
  |\ \  
  | * | add another.txt:1f2b84bfd4029e70d3aeb16a6ecb7f0a0490490e:1dadb4c5c0484717f15a05e2c4fbcf26a134fbd4
  |  /  
  * | add libs submodule:0465a38d195eab5390c82865a90a6cc986a52a72:a860693798b958c292bddba8b9f9c64f5b1f8680
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba:0b7130f9c4103e0b89fd511f432114ef2ebd33e9
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47:1fbe431508b38e48268466d9bb922b979e173ca9
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
