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
  Current branch master is up to date.

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
     bcd5520..dcd1fcd  master     -> origin/master
  Updating bcd5520..dcd1fcd
  Fast-forward
   sub1/file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2
  Current branch master is up to date.

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  1 directory, 2 files

  $ cat sub1/file2
  contents2_repo2

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real/repo2.git" = [':/sub1']
  "real_repo.git" = [':/sub1']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   |-- real%2Frepo2.git
  |   |   |   `-- %3A%2Fsub1
  |   |   |       `-- HEAD
  |   |   `-- real_repo.git
  |   |       `-- %3A%2Fsub1
  |   |           `-- HEAD
  |   `-- upstream
  |       |-- real%2Frepo2.git
  |       |   |-- HEAD
  |       |   `-- refs
  |       |       `-- heads
  |       |           `-- master
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  16 directories, 6 files

