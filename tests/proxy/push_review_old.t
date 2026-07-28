  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git
  $ cd ${TESTTMP}/real_repo
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null
  $ git push 2> /dev/null
$ curl -s http://localhost:8002/flush
Flushed credential cache

  $ cd ${TESTTMP}/sub1

  $ echo contents3 > file3
  $ git add file3
  $ git commit -m "add file3" 1> /dev/null
  $ git log --graph --pretty=%s master
  * add file3
  * add file1
  $ git push origin master:refs/for/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master        
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin refs/for/master:rfm
  From http://localhost:8001/real_repo
   * [new ref]         refs/for/master -> rfm
  $ git checkout rfm
  Switched to branch 'rfm'

  $ git log --graph --pretty=%s master
  * add file2
  * add file1
  $ git log --graph --pretty=%s rfm
  * add file3
  * add file1

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file3
  
  2 directories, 2 files

  $ git rebase master -q
  $ git log --graph --pretty=%s
  * add file3
  * add file2
  * add file1

  $ tree
  .
  `-- sub1
      |-- file1
      |-- file2
      `-- file3
  
  2 directories, 3 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- cache
  |       `-- 32
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
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 81
  |   |   |   `-- b10fb4984d20142cd275b89c91c346e536876a
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- ba
  |   |   |   `-- 7e17233d9f79c96cb694959eb065302acd96a6
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c6
  |   |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
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
      |   |-- 1c
      |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
      |   |-- ad
      |   |   `-- f650cd06e5434fe6deff7639b04c802d63fa5a
      |   |-- b2
      |   |   `-- 6a812a71a431e71d30949f25013ca63f8493c3
      |   |-- info
      |   `-- pack
      |       |-- pack-2f3daab2c2bc0fa3c2cb94022cd73dbeb79fba8b.idx
      |       |-- pack-2f3daab2c2bc0fa3c2cb94022cd73dbeb79fba8b.pack
      |       |-- pack-43b004c57a05484b0057ece370e309a1528a2995.idx
      |       |-- pack-43b004c57a05484b0057ece370e309a1528a2995.pack
      |       |-- pack-a724713a58e1918b9032aed364764a0a4cece84b.idx
      |       `-- pack-a724713a58e1918b9032aed364764a0a4cece84b.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  39 directories, 30 files

  $ cat ${TESTTMP}/josh-proxy.out | grep graph_descendant_of
  [1]
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
