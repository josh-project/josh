Here we are pushing a commit for review that is already in the target branch.
Nothing needs to be done, but this used to cause an error so this is a regression
test for that.

  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/real_repo

  $ echo contents > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents > file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null

  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git vrepo
  $ cd ${TESTTMP}/vrepo
  $ git push origin master:refs/for/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git
   * [new reference]   master -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin refs/for/master:rfm
  From http://localhost:8001/real_repo
   * [new ref]         refs/for/master -> rfm

  $ git log rfm --graph --pretty=%s
  * add file2
  * add file1

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = []
  .
  |-- josh
  |   `-- 17
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
  |   |   |-- 12
  |   |   |   `-- f00e90b6ef79117ce6e650416b8cf517099b78
  |   |   |-- 3e
  |   |   |   `-- 4d66668e6f1dbadc079f36a84768a916bcb8f9
  |   |   |-- 60
  |   |   |   `-- 599f2548a694cee8452bda9c0516027bbbb148
  |   |   |-- 74
  |   |   |   `-- 3f7c56e1cdebc5452c558fea593d48abf45b05
  |   |   |-- 9a
  |   |   |   `-- cea2cd36eb8d8d45cd5399c782d6348a3c8e35
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
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  32 directories, 18 files

