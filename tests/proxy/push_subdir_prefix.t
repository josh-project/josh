  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1:prefix=pre.git pre
  $ cd pre

  $ echo contents2 > pre/file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null
  $ git push 2> /dev/null

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     bb282e9..81b10fb  master     -> origin/master
  Updating bb282e9..81b10fb
  Fast-forward
   sub1/file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2

  $ git-tree-pretty .
  .
  └── sub1/
      ├── file1
      │   ┆  contents1
      └── file2
          ┆  contents2

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1:prefix=pre",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- cache
  |       `-- 33
  |           `-- sled
  |               |-- blobs
  |               |-- conf
  |               `-- db
  |-- mirror
  |   |-- FETCH_HEAD
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
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
      |   |-- 52
      |   |   `-- 2525c3e5980592ddb5eb385ac1262dc6764af3
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- c6
      |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
      |   |-- e6
      |   |   `-- 2cc0b3d612792395dd9ac2ca649da0e6e54620
      |   |-- info
      |   `-- pack
      |       |-- pack-4658096dc6c2a469e11bdb27f1fe57164eddcff2.idx
      |       |-- pack-4658096dc6c2a469e11bdb27f1fe57164eddcff2.pack
      |       |-- pack-683f27aadd9074c55f61bc4efb200f014889512b.idx
      |       |-- pack-683f27aadd9074c55f61bc4efb200f014889512b.pack
      |       |-- pack-f3546ce56eb9276c964f94e9868b52b351242100.idx
      |       `-- pack-f3546ce56eb9276c964f94e9868b52b351242100.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  36 directories, 27 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
