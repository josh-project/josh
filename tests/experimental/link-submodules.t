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
  21b7f3ca0990df3f3bd6affb2d737f6f3b8f0ab2
  [1] :embed=libs
  [2] ::libs/.link.josh
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
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
  040000 tree fae02e4d87ad32ac7f27862dac46f35f83b59692\tlibs (esc)
  100644 blob bcb9dcad21591bd9284afbb6c21e6d69eafe8f15\tmain.txt (esc)

  $ git ls-tree refs/josh/filter/master libs
  040000 tree fae02e4d87ad32ac7f27862dac46f35f83b59692\tlibs (esc)

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
      :prefix=libs
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
  36e64c35239749063d0ff71e5d67b6104a8ec8a4
  [1] :embed=libs
  [2] ::libs/.link.josh
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
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
      :prefix=libs
  ]
  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/josh/filter/master
  ca7ee4a93815d05430f93898d9db88bda94c30a6
  [1] :embed=libs
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] ::libs/.link.josh
  [2] ::modules/another/.link.josh
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
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
  040000 tree 7828c458c886668bae0b12f3b0d914298cee281b\tmodules (esc)

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
  039bfc0995f10cd1bb1b3771b084c1acfca6949e
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [21] sequence_number
  $ git log --graph --pretty=%s:%H refs/josh/filter/master
  *   update libs submodule:039bfc0995f10cd1bb1b3771b084c1acfca6949e
  |\  
  | * add file4.txt:16ee5600999ba1790afdbff89fcbf90c2ddde289
  | * add file3.txt:6a5a648dc487e7006362c19a63c6d432c492cbfb
  * |   add another submodule:ca7ee4a93815d05430f93898d9db88bda94c30a6
  |\ \  
  | * | add another.txt:236993f40f766392481e54ca6cb6c47ed673b25e
  |  /  
  * | add libs submodule:21b7f3ca0990df3f3bd6affb2d737f6f3b8f0ab2
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
  c5ce38ecbae793a62ed4b46bb9f98e09eb71d78c
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [9] :/libs
  [29] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:c5ce38ecbae793a62ed4b46bb9f98e09eb71d78c
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:1eb3174f1a46b64ecb600d9e0626092e57497eae
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link=embedded:/libs:prune=trivial-merge master
  c5ce38ecbae793a62ed4b46bb9f98e09eb71d78c
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  [31] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:c5ce38ecbae793a62ed4b46bb9f98e09eb71d78c
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:1eb3174f1a46b64ecb600d9e0626092e57497eae
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -p --reverse :prune=trivial-merge:export:prefix=libs
  :/libs:export:prune=trivial-merge
  $ josh-filter -s :adapt=submodules:link=embedded:/libs:export:prune=trivial-merge master
  3061af908a0dc1417902fbd7208bb2b8dc354e6c
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
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
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
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
  a62789476855b6c89d1f15ba935d1a937e3ccd67
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [7] :/modules
  [9] :/libs
  [33] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * add another submodule:a62789476855b6c89d1f15ba935d1a937e3ccd67
  * add another.txt:8fbd01fa31551a059e280f68ac37397712feb59e

  $ josh-filter -s :adapt=submodules:link=embedded master --update refs/heads/testsubexport
  039bfc0995f10cd1bb1b3771b084c1acfca6949e
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
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
  * mod libs submodule:e056e2408e26e7970cdfae049b66c302f3bebbbc
  *   update libs submodule:039bfc0995f10cd1bb1b3771b084c1acfca6949e
  |\  
  | * add file4.txt:16ee5600999ba1790afdbff89fcbf90c2ddde289
  | * add file3.txt:6a5a648dc487e7006362c19a63c6d432c492cbfb
  * |   add another submodule:ca7ee4a93815d05430f93898d9db88bda94c30a6
  |\ \  
  | * | add another.txt:236993f40f766392481e54ca6cb6c47ed673b25e
  |  /  
  * | add libs submodule:21b7f3ca0990df3f3bd6affb2d737f6f3b8f0ab2
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ josh-filter -s ":/libs:export:prune=trivial-merge" --update refs/heads/testsubexported
  005cde5c84fbcf17526a0e2fec0a2932c4ce8f24
  [1] :embed=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
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
  3d95fb6ef91b355b85f191cc16ac494a2c9651b3
  [1] :embed=modules/another
  [1] :prefix=modules/another
  [1] :unapply(108820becc39a6a21212e893a7cde9250b564825:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(e1d45e29c75aca63c80d13d82216ccfad39af822:/libs)
  [3] ::libs/.link.josh
  [4] :prefix=libs
  [4] :unapply(ded577ab7e4ed784dfd17e7c2d184fe667e24eae:/libs)
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
  *   update libs submodule:3d95fb6ef91b355b85f191cc16ac494a2c9651b3:0dff040bb48e18b3a89c345d5209fe73b3ca6ed1
  |\  
  | * add file4.txt:16ee5600999ba1790afdbff89fcbf90c2ddde289:368005e9b692ddaf0ca9405d6fbda2b3bb20c8fc
  | * add file3.txt:6a5a648dc487e7006362c19a63c6d432c492cbfb:b152c21838fd1d93f7899907bad5cc9aa245040b
  * |   add another submodule:46aa06004c8da6f31223ac964fb7e4bf36858b98:60bd97919baa32a50192344481784299b405b19a
  |\ \  
  | * | add another.txt:236993f40f766392481e54ca6cb6c47ed673b25e:37e80d7bb93feef02524470d621167dbd5bee9c4
  |  /  
  * | add libs submodule:a4d3b6f7b147345bc19ecfb958fc8dcdcc043b6e:25540e8fa5e62f7524ca3b84396d5ac97abba9d6
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
