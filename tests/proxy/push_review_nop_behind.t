When using nop filter the backward map never gets populated because no translation
is done. This caused a crash when pushing changes that are not fully rebased.
This is a regression test for that problem.

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

  $ git checkout HEAD~1 2> /dev/null

  $ echo contents > file3
  $ git add .
  $ git commit -m "add file3" 1> /dev/null

  $ git push origin HEAD:refs/for/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      JOSH_PUSH -> refs/for/master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git
   * [new branch]      HEAD -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin refs/for/master:rfm
  From http://localhost:8001/real_repo
   * [new ref]         refs/for/master -> rfm

  $ git log rfm --graph --pretty=%s
  * add file3
  * add file1

  $ bash ${TESTDIR}/destroy_test_env.sh
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       `-- %3A
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  11 directories, 3 files
