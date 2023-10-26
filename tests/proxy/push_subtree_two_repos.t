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
  $ git clone -q http://localhost:8001/real/repo2.git real_repo2 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo2
  $ mkdir sub1
  $ echo contents1_repo2 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real/repo2.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git
  $ cd sub1
  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "add file2" 1> /dev/null
  $ git push 1> /dev/null
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    bb282e9..81b10fb  JOSH_PUSH -> master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
     0b4cf6c..d8388f5  master -> master

This uses a repo that has a path with more than one element, causing nested namespaces.
  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real/repo2.git:/sub1.git sub1_repo2
  $ cd sub1_repo2

Put a double slash in the URL to see that it also works
  $ git fetch http://localhost:8002/real//repo2.git:/sub1.git
  From http://localhost:8002/real//repo2.git:/sub1
   * branch            HEAD       -> FETCH_HEAD

  $ git diff HEAD FETCH_HEAD

  $ echo contents2_repo2 > file2
  $ git add file2
  $ git commit -m "add file2" 1> /dev/null
  $ git push 1> /dev/null
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real/repo2.git        
  remote:    bcd5520..dcd1fcd  JOSH_PUSH -> master        
  remote: 
  remote: 
  To http://localhost:8002/real/repo2.git:/sub1.git
     e31c696..5c1144a  master -> master

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
  
  2 directories, 2 files

  $ cat sub1/file2
  contents2

  $ cd ${TESTTMP}/real_repo2
  $ git pull --rebase
  From http://localhost:8001/real/repo2
     bcd5520..dcd1fcd  master     -> origin/master
  Updating bcd5520..dcd1fcd
  Fast-forward
   sub1/file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  2 directories, 2 files

  $ cat sub1/file2
  contents2_repo2

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real/repo2.git" = [
      ":/sub1",
      "::sub1/",
  ]
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- 16
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
  |   |   |-- 4d
  |   |   |   `-- ac9298952aef560bf9691199f53e2b4ff08e3a
  |   |   |-- 92
  |   |   |   `-- 2907f7780152deee70dab1a14810d985391e90
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- bc
  |   |   |   `-- d5520aa5122136789528261b56f05d317a0841
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- cf
  |   |   |   `-- 1e357468e54c4b6234558e00a8fb75c60942a9
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       |-- real%2Frepo2.git
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
      |   |-- 3c
      |   |   `-- cd09de42d21f7e5318bc5eaf258bf82e4103e6
      |   |-- 5c
      |   |   `-- 1144a1e71fe21014b2afec1ed3a3298bd46200
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 81
      |   |   `-- b10fb4984d20142cd275b89c91c346e536876a
      |   |-- 8d
      |   |   `-- 08b32a321a9d226e3b22d01793f62a6c9b22e2
      |   |-- b1
      |   |   `-- 0b7acda365c0c0ed8f7482ccaf201ebbc6dd12
      |   |-- ba
      |   |   `-- 7e17233d9f79c96cb694959eb065302acd96a6
      |   |-- c6
      |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
      |   |-- d8
      |   |   `-- 388f5880393d255b371f1ed9b801d35620017e
      |   |-- dc
      |   |   `-- d1fcde90e4a52777feaf5d7e608f33e5eda53f
      |   |-- e3
      |   |   `-- 1c69658259e37ed5ad682c49a53ec5dc066aec
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  50 directories, 35 files

