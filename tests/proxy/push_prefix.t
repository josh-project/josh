  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:prefix=pre.git pre
  $ cd pre

  $ echo contents2 > pre/file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    bb282e9..b7cf821  JOSH_PUSH -> master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:prefix=pre.git
     1f0b9d8..f99a2dd  master -> master

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     bb282e9..b7cf821  master     -> origin/master
  Updating bb282e9..b7cf821
  Fast-forward
   file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 file2
  Current branch master is up to date.

  $ tree
  .
  |-- file2
  `-- sub1
      `-- file1
  
  1 directory, 2 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub1']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- HEAD
  |   |       `-- %3Aprefix=pre
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  12 directories, 4 files

