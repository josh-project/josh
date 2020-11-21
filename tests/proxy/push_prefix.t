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
  remote:  To http://localhost:8001/real_repo.git        
  remote:    *..*  JOSH_PUSH -> master         (glob)
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:prefix=pre.git
     *..*  master -> master (glob)

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     *..*  master     -> origin/master (glob)
  Updating *..* (glob)
  Fast-forward
   file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 file2

  $ tree
  .
  |-- file2
  `-- sub1
      `-- file1
  
  1 directory, 2 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3Aprefix=pre
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  14 directories, 3 files

