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
  $ git push origin master:refs/for/master%topic=mytopic
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      JOSH_PUSH -> refs/for/master%topic=mytopic        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      master -> refs/for/master%topic=mytopic

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   `-- filtered
  |       `-- real_repo.git
  |           `-- #%sub1
  |               `-- heads
  |                   `-- master
  |-- namespaces
  |   `-- real_repo.git
  |       `-- refs
  |           `-- heads
  |               `-- master
  `-- tags
  
  11 directories, 2 files

$ cat ${TESTTMP}/josh-proxy.out
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
