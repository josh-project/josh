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
  d165a029bc6a408988d4a83e4dc14f7aa5bed51c
  [1] :embed=libs
  [2] ::libs/.link.josh
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
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
  040000 tree 5437f2979cd4a01a7b852da7199825c45cd8347a\tlibs (esc)
  100644 blob bcb9dcad21591bd9284afbb6c21e6d69eafe8f15\tmain.txt (esc)

  $ git ls-tree refs/josh/filter/master libs
  040000 tree 5437f2979cd4a01a7b852da7199825c45cd8347a\tlibs (esc)

  $ git ls-tree refs/josh/filter/master libs/foo
  040000 tree 81a0b9c71d7fac4f553b2a52b9d8d52d07dd8036\tlibs/foo (esc)

  $ git ls-tree refs/josh/filter/master libs/bar
  040000 tree bd42a3e836f59dda9f9d5950d0e38431c9b1bfb5\tlibs/bar (esc)

  $ git show refs/josh/filter/master:libs/.link.josh
  :~(
      commit="00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd"
      remote="../submodule-repo"
      target="HEAD"
  )[
      :/
  ] (no-eol)
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
  63de7227a11c0aaee2179218bd3989100d491f80
  [1] :embed=libs
  [2] ::libs/.link.josh
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
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
      remote="../submodule-repo"
      target="HEAD"
  )[
      :/
  ] (no-eol)
  $ josh-filter -s :adapt=submodules:link master --update refs/josh/filter/master
  3bbebd1bcefb150a160089776ac384f5402c092f
  [1] :embed=libs
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] ::libs/.link.josh
  [2] ::modules/another/.link.josh
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
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
  040000 tree 5a77d54d7f3785f5e45c494ab618a1bf1cbaa7f0\tmodules (esc)

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
  5265f6775d830084c12c9ff59566eac5a934c7c7
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [21] sequence_number
  $ git log --graph --pretty=%s:%H refs/josh/filter/master
  *   update libs submodule:5265f6775d830084c12c9ff59566eac5a934c7c7
  |\  
  | * add file4.txt:6d3319e851d3799d989e7ecc6d12a2ddda5aac8d
  | * add file3.txt:6bb30665285c7d39156f8ea3e74c79a53672710e
  * |   add another submodule:3bbebd1bcefb150a160089776ac384f5402c092f
  |\ \  
  | * | add another.txt:f6b1aa76c1352b0cab300f78f550ac4058671c5b
  |  /  
  * | add libs submodule:d165a029bc6a408988d4a83e4dc14f7aa5bed51c
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ git ls-tree --name-only -r 5265f6775d830084c12c9ff59566eac5a934c7c7
  libs/.link.josh
  libs/bar/file3.txt
  libs/bar/file4.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  libs/foo/file3.txt
  main.txt
  modules/another/.link.josh
  modules/another/another.txt
  $ git ls-tree --name-only -r 6d3319e851d3799d989e7ecc6d12a2ddda5aac8d
  libs/bar/file3.txt
  libs/bar/file4.txt
  libs/foo/file1.txt
  libs/foo/file2.txt
  libs/foo/file3.txt
  main.txt
  modules/another/.link.josh
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

  $ josh-filter -s :adapt=submodules:link:/libs master
  8ac312666f3f1871775f85d9f05a74f381021623
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [9] :/libs
  [29] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:8ac312666f3f1871775f85d9f05a74f381021623
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:628bd2a818c8443489290e9d262bebd7f0234763
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -s :adapt=submodules:link:/libs:prune=trivial-merge master
  8ac312666f3f1871775f85d9f05a74f381021623
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :link=embedded
  [6] :prune=trivial-merge
  [9] :/libs
  [31] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  *   update libs submodule:8ac312666f3f1871775f85d9f05a74f381021623
  |\  
  | * add file4.txt:3061af908a0dc1417902fbd7208bb2b8dc354e6c
  | * add file3.txt:411907f127aa115588a614ec1dff6ee3c4696173
  * | add libs submodule:628bd2a818c8443489290e9d262bebd7f0234763
  |/  
  * add bar with file:00c8fe9f1bb75a3f6280992ec7c3c893d858f5dd
  * add foo with files:4b63f3e50a3a34404541bc4519a3a1a0a8e6f738
  $ josh-filter -p --reverse :prune=trivial-merge:export:prefix=libs
  :/libs:export:prune=trivial-merge
  $ josh-filter -s :adapt=submodules:link:/libs:export:prune=trivial-merge master
  3061af908a0dc1417902fbd7208bb2b8dc354e6c
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
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
  $ josh-filter -s :adapt=submodules:link:/libs:export:prune=trivial-merge master
  3061af908a0dc1417902fbd7208bb2b8dc354e6c
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
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
  $ josh-filter -s :adapt=submodules:link:/modules/another master
  10045f12b3df5f3046513baf4a39559ab29df4c5
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
  [5] :"{@}"
  [5] :adapt=submodules
  [5] :export
  [5] :link=embedded
  [6] :prune=trivial-merge
  [7] :/modules
  [9] :/libs
  [33] sequence_number
  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * add another submodule:10045f12b3df5f3046513baf4a39559ab29df4c5
  * add another.txt:8fbd01fa31551a059e280f68ac37397712feb59e

  $ josh-filter -s :adapt=submodules:link master --update refs/heads/testsubexport
  5265f6775d830084c12c9ff59566eac5a934c7c7
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
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
  * mod libs submodule:66abaa06d05103eeaa3ebcc20a1bce111a596d29
  *   update libs submodule:5265f6775d830084c12c9ff59566eac5a934c7c7
  |\  
  | * add file4.txt:6d3319e851d3799d989e7ecc6d12a2ddda5aac8d
  | * add file3.txt:6bb30665285c7d39156f8ea3e74c79a53672710e
  * |   add another submodule:3bbebd1bcefb150a160089776ac384f5402c092f
  |\ \  
  | * | add another.txt:f6b1aa76c1352b0cab300f78f550ac4058671c5b
  |  /  
  * | add libs submodule:d165a029bc6a408988d4a83e4dc14f7aa5bed51c
  |\| 
  | * add bar with file:2926fa3361cec2d5695a119fcc3592f4214af3ba
  | * add foo with files:e975fd8cd3f2d2de81884f5b761cc0ac150bdf47
  * add main.txt:c404a74092888a14d109c8211576d2c50fc2affd
  * init:01d3837a9f7183df88e956cc81f085544f9c6563
  $ josh-filter -s ":/libs:export:prune=trivial-merge" --update refs/heads/testsubexported
  005cde5c84fbcf17526a0e2fec0a2932c4ce8f24
  [1] :embed=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
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

  $ josh-filter -s ":adapt=submodules:link:unlink" refs/heads/master --update refs/heads/unlinked_master
  2b905ecdb05cd2b5a248c5d6e7f3dd6887095a74
  [1] :embed=modules/another
  [1] :prefix=modules/another
  [1] :unapply(71fd9c59b40956e00bf73908266692bb0e266df3:/modules/another)
  [2] :/another
  [2] ::modules/another/.link.josh
  [2] :embed=libs
  [2] :unapply(dcfe52c799660f1611d6125542d0503230a5e14b:/libs)
  [3] ::libs/.link.josh
  [4] :prefix=libs
  [4] :unapply(c47d7c6dbbd79ffc66035eb776fb753e0ec74276:/libs)
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
  *   update libs submodule:2b905ecdb05cd2b5a248c5d6e7f3dd6887095a74:0dfb828fc97a6684c6e0532dfdf0fe6a5005768c
  |\  
  | * add file4.txt:6d3319e851d3799d989e7ecc6d12a2ddda5aac8d:148a6ede3adb68f6213a2b209e68afb3ee99bf15
  | * add file3.txt:6bb30665285c7d39156f8ea3e74c79a53672710e:52778dc1b3e31b5fcc027bbf2f9b48319da95b00
  * |   add another submodule:55aaec6b3d26535f20f50efc1dd8c5e4937f4d59:6d250abbd449d5fd76e0f7ff44a659e6ec5136a3
  |\ \  
  | * | add another.txt:f6b1aa76c1352b0cab300f78f550ac4058671c5b:0b1caa6663b6f1f8c31d81bf8d3ad631ec7a6c80
  |  /  
  * | add libs submodule:2f033b1d40c13a99f3229902b4909f1182051ba5:780209fedc3c886da1b1e6e87c7962f20b817595
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
