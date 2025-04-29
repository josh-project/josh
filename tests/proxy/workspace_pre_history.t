Workspaces should also contain the history of the main directory before the workspace.josh
file was created


  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.


  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir ws

  $ echo content1 > ws/file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add ws
  $ git commit -m "add workspace" 1> /dev/null

  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file1
  * add workspace
  * initial


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  $ cd ws
  $ tree
  .
  |-- c
  |   `-- subsub
  |       `-- file1
  |-- file1
  `-- workspace.josh
  
  3 directories, 3 files

  $ git log --graph --pretty=%s
  * add file1
  * add workspace
  * initial

  $ git checkout -q HEAD~1 1> /dev/null

  $ tree
  .
  |-- file1
  `-- workspace.josh
  
  1 directory, 2 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub1/",
      "::sub1/subsub/",
      "::ws/",
      ":workspace=ws",
  ]
  .
  |-- josh
  |   `-- 22
  |       `-- sled
  |           |-- blobs
  |           |-- conf
  |           `-- db
  |-- mirror
  |   |-- FETCH_HEAD
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- 0d
  |   |   |   `-- aea91dddfc42dea2123d0d83c6a21470e5e0e9
  |   |   |-- 0f
  |   |   |   `-- 83fa83f707ea8a648efd7f701e9147c170acad
  |   |   |-- 1c
  |   |   |   `-- cf878cdccad4c0de7f9530fa317f8e068de98a
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 68
  |   |   |   `-- 562b26584f1088791b2dd81a974b7ecf4618d5
  |   |   |-- 72
  |   |   |   `-- c107ae37a43953a886b61a97c72bfaa1b86e8b
  |   |   |-- 95
  |   |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a4
  |   |   |   `-- c6dcc717e6ba84bbe41c7dce76349e8bd39d0d
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- f5
  |   |   |   `-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |-- fc
  |   |   |   `-- ec311dce010e6eb21b79097eecc7cb8d70eeda
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               `-- heads
  |       |                   `-- master
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 06
      |   |   `-- f56dd7e7b5abf977267595ccfc3f1a5f1eea28
      |   |-- 27
      |   |   `-- 5b45aec0a1c944c3a4c71cc71ee08d0c9ea347
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 4f
      |   |   `-- be5e3e9300ad2318545b9b8197029d55ac5395
      |   |-- 78
      |   |   `-- 2f6261fa32f8bfec7b89f77bb5cce40c4611cb
      |   |-- 98
      |   |   `-- 84cc2efe368ea0aa9d912fa596b26c5d75dbee
      |   |-- 9c
      |   |   `-- f258b407cd9cdba97e16a293582b29d302b796
      |   |-- a8
      |   |   `-- 6544ef29b946481d26cb4cfb55844342069c0e
      |   |-- b6
      |   |   `-- c8440fe2cd36638ddb6b3505c1e8f2202f6191
      |   |-- eb
      |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
      |   |-- f8
      |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  51 directories, 37 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
