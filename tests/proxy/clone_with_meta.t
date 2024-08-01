  $ export JOSH_META_REPO=/meta_repo.git
  $ . ${TESTDIR}/setup_test_env.sh

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/meta_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd meta_repo
  $ mkdir -p path/to/my_repo.git
  $ mkdir -p with_prefix.git

  $ cat > path/to/my_repo.git/config.yml <<EOF
  > repo: /real_repo.git
  > EOF

  $ cat > with_prefix.git/config.yml <<EOF
  > repo: /real_repo.git
  > filter: :prefix=my_prefix
  > EOF

  $ git add .
  $ git commit -m "add my_repo"
  [master (root-commit) 488d229] add my_repo
   2 files changed, 3 insertions(+)
   create mode 100644 path/to/my_repo.git/config.yml
   create mode 100644 with_prefix.git/config.yml
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
  
  3 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * add file1

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/with_prefix.git
  $ cd with_prefix
  $ tree
  .
  `-- my_prefix
      |-- sub1
      |   `-- file1
      `-- sub2
          `-- file2
  
  4 directories, 2 files

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/with_prefix.git:/my_prefix/sub1.git
  $ cd sub1
  $ tree
  .
  `-- file1
  
  1 directory, 1 file


  $ bash ${TESTDIR}/destroy_test_env.sh
  "meta_repo.git" = [
      "::path/",
      "::path/to/",
      "::with_prefix.git/",
  ]
  "real_repo.git" = [
      "::sub1/",
      "::sub2/",
      ":prefix=my_prefix",
      ":prefix=my_prefix:/my_prefix:/sub1",
  ]
  .
  |-- josh
  |   `-- 20
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
  |   |   |   `-- d0327bf3dda29d1ca87d64b4913431f1557110
  |   |   |-- 0e
  |   |   |   `-- 573a1bd81cc9aefaf932187b9e68a1052a4ff6
  |   |   |-- 23
  |   |   |   `-- 87c32648eefdee78386575672ac091da849b08
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 48
  |   |   |   `-- 8d229fd9dfaadbc6b9e2d91000513653c1ec65
  |   |   |-- 51
  |   |   |   `-- 5bd921f427d8fa60385d5922743f299c360aaf
  |   |   |-- 61
  |   |   |   `-- a7852c0c4aacc0c0a1bcf81454b6ca64cff497
  |   |   |-- 6b
  |   |   |   `-- ae79b931ab8c5822e07f48793068eb748e2a13
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a6
  |   |   |   `-- b38a805ee48896f37a9eb5b0a1bac52c2a8009
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- e3
  |   |   |   `-- b357df20c48de2f49ad7d125e8a564e19fe4d6
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
      |   |-- 0b
      |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
      |   |-- 5f
      |   |   `-- d3fa6f1f9fae3965027f948f68c7c1919bab22
      |   |-- a8
      |   |   `-- 53d3b05291c3f530657aa935b51d11369479ae
      |   |-- b5
      |   |   `-- ec4ca5a3d90abd48a9ee076b8bf195362f6327
      |   |-- fb
      |   |   `-- 5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  50 directories, 35 files
