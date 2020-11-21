  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git
  $ cd ${TESTTMP}/real_repo
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null
  $ git push 2> /dev/null
  $ curl -s http://localhost:8002/flush
  Flushed credential cache

  $ cd ${TESTTMP}/sub1

  $ echo contents3 > file3
  $ git add file3
  $ git commit -m "add file3" 1> /dev/null
  $ git log --graph --pretty=%s master
  * add file3
  * add file1
  $ git push origin master:refs/for/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin refs/for/master:rfm
  From http://localhost:8001/real_repo
   * [new ref]         refs/for/master -> rfm
  $ git checkout rfm
  Switched to branch 'rfm'

  $ git log --graph --pretty=%s master
  * add file2
  * add file1
  $ git log --graph --pretty=%s rfm
  * add file3
  * add file1

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file3
  
  1 directory, 2 files

  $ git rebase master
  Rebasing (1/1)\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[KSuccessfully rebased and updated refs/heads/rfm. (esc)
  $ git log --graph --pretty=%s
  * add file3
  * add file2
  * add file1

  $ tree
  .
  `-- sub1
      |-- file1
      |-- file2
      `-- file3
  
  1 directory, 3 files

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

  $ cat ${TESTTMP}/josh-proxy.out | grep graph_descendant_of
  [1]
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
