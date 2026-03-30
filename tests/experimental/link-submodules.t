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

  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/josh/filter/master
  de1d866c061edd259d604b67b87e8f9bbb95a493
  [1] :embed=libs
  [2] ::libs/.link.josh
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
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
  libs/.link.josh
  libs/bar/file3.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  main.txt

  $ git ls-tree refs/josh/filter/master
  040000 tree c8039cb1eba006845979264c512b5b662a5e7d97\tlibs (esc)
  100644 blob bcb9dcad21591bd9284afbb6c21e6d69eafe8f15\tmain.txt (esc)

  $ git ls-tree refs/josh/filter/master libs
  040000 tree c8039cb1eba006845979264c512b5b662a5e7d97\tlibs (esc)

  $ git ls-tree refs/josh/filter/master libs/foo
  040000 tree 81a0b9c71d7fac4f553b2a52b9d8d52d07dd8036\tlibs/foo (esc)

  $ git ls-tree refs/josh/filter/master libs/bar
  040000 tree bd42a3e836f59dda9f9d5950d0e38431c9b1bfb5\tlibs/bar (esc)

  $ git show refs/josh/filter/master:libs/.link.josh
  :~(
      commit="00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd"
      mode="embedded"
      remote="../submodule-repo"
      target="HEAD"
  )[
      :/
  ]
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
  4e48d5817374b1950f15c4a8d135863594596dfe
  [1] :embed=libs
  [2] ::libs/.link.josh
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] :"{@}"
  [3] :link=embedded
  [4] :adapt=submodules
  [11] sequence_number
  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/.link.josh
  main.txt
  modules/another/.link.josh
  $ git show refs/josh/filter/master:libs/.link.josh
  :~(
      commit="00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd"
      mode="pointer"
      remote="../submodule-repo"
      target="HEAD"
  )[
      :/
  ]
  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/josh/filter/master
  c53a43b8e6335f332d28aac2b1aa5db511ef639e
  [1] :embed=libs
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] ::libs/.link.josh
  [2] ::modules/another/.link.josh
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
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
  libs/.link.josh
  libs/bar/file3.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  main.txt
  modules/another/.link.josh
  modules/another/another.txt

  $ git ls-tree refs/josh/filter/master modules
  040000 tree 7b3cd056b887d0fee64b0752ce0bd983a5e05701\tmodules (esc)

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


  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/josh/filter/master
  e08fc8b154ffccae088368b570905d60f1262406
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [21] sequence_number
  $ git log --graph --pretty=%s:%H refs/josh/filter/master
  *   update libs submodule:e08fc8b154ffccae088368b570905d60f1262406
  |\  
  | * add file4.txt:7a5759d4b084f1e808874a44032a0b6446ddaaea
  | * add file3.txt:c1b73bbd9c482eaca93ef84e9507e78fef85634d
  * |   add another submodule:c53a43b8e6335f332d28aac2b1aa5db511ef639e
  |\ \  
  | * | add another.txt:dfe454879200a2e8a9698c8b735f19afc246a5f4
  |  /  
  * | add libs submodule:de1d866c061edd259d604b67b87e8f9bbb95a493
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ git ls-tree --name-only -r 5265f6775d830084c12c9ff59566eac5a934c7c7
  fatal: not a tree object
  [128]
  $ git ls-tree --name-only -r 6d3319e851d3799d989e7ecc6d12a2ddda5aac8d
  fatal: not a tree object
  [128]
  $ git ls-tree --name-only -r refs/josh/filter/master
  libs/.link.josh
  libs/bar/file3.txt
  libs/bar/file4.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  libs/foo/file3.txt
  main.txt
  modules/another/.link.josh
  modules/another/another.txt

  $ git show refs/josh/filter/master:libs/foo/file3.txt
  new content

  $ git show refs/josh/filter/master:libs/bar/file4.txt
  another new content

  $ josh-filter -s :adapt=submodules:link=embedded:/libs master
  b2e90aae4a2c0e7c2f9476d50300f92546cbc58f
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [9] :/libs
  [29] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:b2e90aae4a2c0e7c2f9476d50300f92546cbc58f
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:24c4ad0143802f7d0cb1d76c5cfbc108f2f09e1e
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link=embedded:/libs:prune=trivial-merge master
  b2e90aae4a2c0e7c2f9476d50300f92546cbc58f
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  [31] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:b2e90aae4a2c0e7c2f9476d50300f92546cbc58f
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:24c4ad0143802f7d0cb1d76c5cfbc108f2f09e1e
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -p --reverse :prune=trivial-merge:export:prefix=libs
  :/libs:export:prune=trivial-merge
  $ josh-filter -s :adapt=submodules:link=embedded:/libs:export:prune=trivial-merge master
  3061af908a0dc1417902fbd7208bb2b8dc354e6c
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  [31] sequence_number
  $ git log --graph --pretty=%s:%H:%T FILTERED_HEAD
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c:ac420a625dfb874002210e623a7fdb55708ef2fa
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173:2935d839ce5e2fa8d5d8fb1a8541bf95b98fbedb
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd:e06b912df6ae0105e3a525f7a9427d98574fbc4f
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738:7ca6af9b9a7d0d7f4723a74cf6006c14eaea547e
  $ josh-filter -s :adapt=submodules:link=embedded:/libs:export:prune=trivial-merge master
  3061af908a0dc1417902fbd7208bb2b8dc354e6c
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  [31] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link=embedded:/modules/another master
  b74da266394ae690a9b37b003779a1b59373bc65
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [7] :/modules
  [9] :/libs
  [33] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * add another submodule:b74da266394ae690a9b37b003779a1b59373bc65
  * add another.txt:8fbd01fa31551a059e280f68ac37397712feb59e

  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/heads/testsubexport
  e08fc8b154ffccae088368b570905d60f1262406
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [7] :/modules
  [9] :/libs
  [33] sequence_number
  $ rm -Rf libs
  $ rm -Rf modules
  $ git checkout testsubexport
  Switched to branch 'testsubexport'
  $ echo "fo" > libs/foobar.txt
  $ git add .
  $ git commit -m "mod libs submodule" 1> /dev/null
  $ git log --graph --pretty=%s:%H
  * mod libs submodule:36043860d4b2efb245881c8bcb6e03e1b955c207
  *   update libs submodule:e08fc8b154ffccae088368b570905d60f1262406
  |\  
  | * add file4.txt:7a5759d4b084f1e808874a44032a0b6446ddaaea
  | * add file3.txt:c1b73bbd9c482eaca93ef84e9507e78fef85634d
  * |   add another submodule:c53a43b8e6335f332d28aac2b1aa5db511ef639e
  |\ \  
  | * | add another.txt:dfe454879200a2e8a9698c8b735f19afc246a5f4
  |  /  
  * | add libs submodule:de1d866c061edd259d604b67b87e8f9bbb95a493
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ josh-filter -s ":/libs:export:prune=trivial-merge" --update refs/heads/testsubexported
  005cde5c84fbcf17526a0e2fec0a2932c4ce8f24
  [1] :embed=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :export
  [7] :/modules
  [7] :prune=trivial-merge
  [10] :/libs
  [36] sequence_number

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

  $ josh-filter -s ":adapt=submodules:link=embedded:unlink" refs/heads/master --update refs/heads/unlinked_master
  765042a09b160bb584461699a555d7d0dc812ec0
  [1] :embed=modules/another
  [1] :prefix=modules/another
  [1] :unapply(c6f07ba88c247cc089ba6729a95516729253e168:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(1f0a2b6933f2c095b7e3b6e958cf3695538b9f42:/libs)
  [3] ::libs/.link.josh
  [4] :prefix=libs
  [4] :unapply(c84fadadb70fa40a33a61a03b334d2995b67b497:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :export
  [7] :/modules
  [7] :prune=trivial-merge
  [10] :/libs
  [10] :unlink
  [36] sequence_number

  $ git log --graph --pretty=%s:%H:%T refs/heads/unlinked_master
  *   update libs submodule:765042a09b160bb584461699a555d7d0dc812ec0:7ff0acd41cae0c10ec166888f075e3f22ad8b299
  |\  
  | * add file4.txt:7a5759d4b084f1e808874a44032a0b6446ddaaea:81be2dc8fa07e51134c28c4570c4333c0db0dec7
  | * add file3.txt:c1b73bbd9c482eaca93ef84e9507e78fef85634d:213a615bad5b0a49101dab6b84faea148c41b378
  * |   add another submodule:e50aafacef8f49c22bb6569952c060dfb82b8fe0:7615aa4b11b195001ea4befbe65a098cf51fe992
  |\ \  
  | * | add another.txt:dfe454879200a2e8a9698c8b735f19afc246a5f4:d469e2eb543c1ea2388eb37085326e64606183a9
  |  /  
  * | add libs submodule:07cd9b8b539e998af1ceceb1e0082f4c15c147e5:6e719f77f0079c314bf25ee0267e5c740a43e0b2
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

  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/josh/filter/master
  62f270988ac3ab05a33d0705fb1e4982165f69f2
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
