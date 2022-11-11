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

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  1 directory, 2 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub1:prefix=pre',
  ]
  .
  |-- josh
  |   `-- 14
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
      |   |-- 0b
      |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
      |   |-- 32
      |   |   `-- bc15e3ec04d5d9d2a216055ea51bafb2249877
      |   |-- 50
      |   |   `-- bcd696c2041bb31c0a9a1111dfa2686fc6fea6
      |   |-- 52
      |   |   `-- 2525c3e5980592ddb5eb385ac1262dc6764af3
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 81
      |   |   `-- b10fb4984d20142cd275b89c91c346e536876a
      |   |-- ba
      |   |   `-- 7e17233d9f79c96cb694959eb065302acd96a6
      |   |-- c6
      |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
      |   |-- d8
      |   |   `-- 388f5880393d255b371f1ed9b801d35620017e
      |   |-- e6
      |   |   `-- 2cc0b3d612792395dd9ac2ca649da0e6e54620
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  40 directories, 27 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
