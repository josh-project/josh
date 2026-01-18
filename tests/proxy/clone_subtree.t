  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) bb282e9] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2"
  [master ffe8d08] add file2
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file2

  $ tree
  .
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- file2
  
  3 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * add file1

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ git ls-remote --symref
  From http://localhost:8001/real_repo.git
  ref: refs/heads/master\tHEAD (esc)
  ffe8d082c1034053534ea8068f4205ac72a1098e\tHEAD (esc)
  ffe8d082c1034053534ea8068f4205ac72a1098e\trefs/heads/master (esc)

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git sub1

  $ cd sub1
  $ git ls-remote --symref
  From http://localhost:8002/real_repo.git:/sub1.git
  ref: refs/heads/master\tHEAD (esc)
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\tHEAD (esc)
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\trefs/heads/master (esc)
  $ cat .git/refs/remotes/origin/HEAD
  ref: refs/remotes/origin/master

  $ tree
  .
  `-- file1
  
  1 directory, 1 file

  $ git log --graph --pretty=%s
  * add file1

  $ cat file1
  contents1

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
      "::sub2/",
  ]
  .
  |-- josh
  |   `-- 26
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
  |   |   |-- 23
  |   |   |   `-- 87c32648eefdee78386575672ac091da849b08
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- ff
  |   |   |   `-- e8d082c1034053534ea8068f4205ac72a1098e
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
      |   |-- 0b
      |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  35 directories, 21 files
