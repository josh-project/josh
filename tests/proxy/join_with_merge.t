  $ . ${TESTDIR}/setup_test_env.sh


  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ git checkout -b master
  Switched to a new branch 'master'
  $ echo content1 > file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ git checkout -b new1
  Switched to a new branch 'new1'
  $ echo content > newfile1 1> /dev/null
  $ git add .
  $ git commit -m "add newfile1" 1> /dev/null

  $ git checkout master 1> /dev/null
  Switched to branch 'master'
  $ echo content > newfile_master 1> /dev/null
  $ git add .
  $ git commit -m "newfile master" 1> /dev/null

  $ git merge -q new1 --no-ff

  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ git fetch --force http://localhost:8002/real_repo.git:prefix=sub1.git master:joined 1> /dev/null
  From http://localhost:8002/real_repo.git:prefix=sub1
   * [new branch]      master     -> joined

  $ git checkout joined
  Switched to branch 'joined'

  $ git log --graph --pretty=%s
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial

  $ tree
  .
  `-- sub1
      |-- file1
      |-- newfile1
      `-- newfile_master
  
  2 directories, 3 files


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [":prefix=sub1"]
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
  |   |   |-- 0f
  |   |   |   `-- 7ceed53e5b4ab96efad3c0b77e2c00d10169ba
  |   |   |-- 41
  |   |   |   `-- 8fcc975168e0bfc9dd53bbb98f740da2e983c0
  |   |   |-- 53
  |   |   |   `-- 9f411b73b3c22bc218bece495a841880fd4e2c
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- b7
  |   |   |   `-- 85a0b60f6ef7044b4c59c318e18e2c47686085
  |   |   |-- c6
  |   |   |   `-- 6fb92e3be8e4dc4c89f94d796f3a4b1833e0fa
  |   |   |-- e4
  |   |   |   `-- 5f0325cd9fab82d962b758e556d9bf8079fc37
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- eb
  |   |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
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
      |   |-- 22
      |   |   `-- b22885799153b100fdb3445c59c4c3f4482ed3
      |   |-- 24
      |   |   `-- 6b71ecf742c084ec41ba6f2c4e355ca4806ef0
      |   |-- 5a
      |   |   `-- 29d3ef74b34ca534513c6499e9c9011371fab4
      |   |-- 68
      |   |   `-- 9a59b2fad33839a7e8d6c1c02ce94095d8fe1e
      |   |-- 85
      |   |   `-- 352a4584261ab3b54e32aa4ddfe04e80c6800e
      |   |-- 8e
      |   |   `-- 9aa8ffc35bbc452f9654b834047168ce02dc48
      |   |-- c3
      |   |   `-- 0a8aef421831a8492d21efde601e90a742544f
      |   |-- db
      |   |   `-- dfb5702a23dd0a3502f4d6ce7287a3a4d85abe
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  44 directories, 30 files
