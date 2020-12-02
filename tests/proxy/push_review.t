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

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git
  $ cd sub1

  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "add file2" 1> /dev/null
  $ git push origin master:refs/for/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/for/master
  $ git push origin master:refs/drafts/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:  * [new reference]   JOSH_PUSH -> refs/drafts/master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/drafts/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin refs/for/master:rfm
  From http://localhost:8001/real_repo
   * [new ref]         refs/for/master -> rfm
  $ git fetch origin refs/drafts/master:rdm
  From http://localhost:8001/real_repo
   * [new ref]         refs/drafts/master -> rdm
  $ git checkout rfm
  Switched to branch 'rfm'

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  1 directory, 2 files

  $ cat sub1/file2
  contents2

  $ git checkout rdm
  Switched to branch 'rdm'

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
  |                   `-- master
  |-- namespaces
  `-- tags
  
  12 directories, 2 files

$ cat ${TESTTMP}/josh-proxy.out
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
