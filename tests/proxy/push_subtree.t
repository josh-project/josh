  $ source ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real_repo.git &> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null
  $ git push &> /dev/null

  $ cd ${TESTTMP}

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git:/sub1.git
  $ cd sub1

  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "add file2" &> /dev/null
  $ git push origin HEAD:refs/heads/new_branch
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      JOSH_PUSH -> new_branch        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      HEAD -> new_branch
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:    *..*  JOSH_PUSH -> master* (glob)
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
     *..*  master -> master (glob)

  $ cd ${TESTTMP}/real_repo
  $ git pull
  From http://localhost:8001/real_repo
     *..*  master     -> origin/master (glob)
   * [new branch]      new_branch -> origin/new_branch
  Updating *..* (glob)
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

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       `-- %3A%2Fsub1
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   |-- master
  |                   `-- new_branch
  `-- tags
  
  11 directories, 3 files

$ cat ${TESTTMP}/josh-proxy.out
