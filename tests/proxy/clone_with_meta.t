  $ export JOSH_META_REPO=/meta_repo.git
  $ . ${TESTDIR}/setup_test_env.sh

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/meta_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd meta_repo
  $ mkdir -p path/to/my_repo.git

  $ cat > path/to/my_repo.git/repo.yml <<EOF
  > repo: /real_repo.git
  > EOF

  $ git add .
  $ git commit -m "add my_repo"
  [master (root-commit) fc1c140] add my_repo
   1 file changed, 1 insertion(+)
   create mode 100644 path/to/my_repo.git/repo.yml
  $ git push
  To http://localhost:8001/meta_repo.git
   * [new branch]      master -> master


  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git backing_repo
  warning: You appear to have cloned an empty repository.

  $ cd backing_repo

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

  $ git log --graph --pretty=%s
  * add file2
  * add file1

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8002/real_repo.git
  remote: meta repo entry not found
  fatal: unable to access 'http://localhost:8002/real_repo.git/': The requested URL returned error: 500
  [128]

  $ git clone -q http://localhost:8002/path/to/my_repo.git
  $ cd my_repo
  $ git ls-remote --symref
  From http://localhost:8002/path/to/my_repo.git
  ref: refs/heads/master\tHEAD (esc)
  ffe8d082c1034053534ea8068f4205ac72a1098e\tHEAD (esc)
  ffe8d082c1034053534ea8068f4205ac72a1098e\trefs/heads/master (esc)
  $ cat .git/refs/remotes/origin/HEAD
  ref: refs/remotes/origin/master

  $ tree
  .
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- file2
  
  2 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * add file1


  $ bash ${TESTDIR}/destroy_test_env.sh
  "meta_repo.git" = [
      ':/path',
      ':/path/to',
  ]
  "real_repo.git" = [
      ':/sub1',
      ':/sub2',
  ]
  .
  |-- josh
  |   `-- 12
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
  |   |   |-- 0e
  |   |   |   `-- 573a1bd81cc9aefaf932187b9e68a1052a4ff6
  |   |   |-- 1d
  |   |   |   `-- ffbbd63f1d894f194cf0bd16a3f19b82269b53
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
  |   |   |-- c9
  |   |   |   `-- e952f9beba7da8de8ae3b350f15e3774645a54
  |   |   |-- de
  |   |   |   `-- 47e9b40111cce1577ab928e7c1ac57b41ee9b7
  |   |   |-- ec
  |   |   |   `-- 6006d85ed823a63d900cc7a0ed534ce3b8b5c4
  |   |   |-- fc
  |   |   |   `-- 1c140115774c6181f8c8d336e1714590f53230
  |   |   |-- ff
  |   |   |   `-- e8d082c1034053534ea8068f4205ac72a1098e
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       |-- meta_repo.git
  |       |       |   |-- HEAD
  |       |       |   `-- refs
  |       |       |       `-- heads
  |       |       |           `-- master
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
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  42 directories, 28 files
