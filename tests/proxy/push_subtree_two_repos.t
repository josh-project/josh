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
  remote:  To http://localhost:8001/real_repo.git        
  remote:    *..*  JOSH_PUSH -> master * (glob)
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
     *..*  master -> master (glob)

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
  remote:  To http://localhost:8001/real/repo2.git        
  remote:    *..*  JOSH_PUSH -> master * (glob)
  remote: 
  remote: 
  To http://localhost:8002/real/repo2.git:/sub1.git
     *..*  master -> master (glob)

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     *..*  master     -> origin/master (glob)
  Updating *..* (glob)
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

  $ cat sub1/file2
  contents2

  $ cd ${TESTTMP}/real_repo2
  $ git pull --rebase
  From http://localhost:8001/real/repo2
     *..*  master     -> origin/master (glob)
  Updating *..* (glob)
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

  $ cat sub1/file2
  contents2_repo2

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   |-- real%2Frepo2.git
  |   |   |   `-- %3A%2Fsub1
  |   |   |       `-- heads
  |   |   |           `-- master
  |   |   `-- real_repo.git
  |   |       `-- %3A%2Fsub1
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       |-- real%2Frepo2.git
  |       |   `-- refs
  |       |       `-- heads
  |       |           `-- master
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  18 directories, 4 files

