# setup with caching
  $ EXTRA_OPTS=--cache-duration\ 100 . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.
  $ curl -s http://localhost:8002/version
  Version: 22.4.15
  $ cd real_repo
  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir sub1
  $ echo content1 > sub1/file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master
  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git

# test with caching
  $ cd sub1
  $ echo "content2" > file2
  $ git add .
  $ git commit -m "add file 2"
  [master bdc926c] add file 2
   1 file changed, 1 insertion(+)
   create mode 100644 file2
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    5a29d3e..2e310c4  JOSH_PUSH -> master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
     eb6a311..bdc926c  master -> master
# funny but expected behaviour
  $ git fetch
  From http://localhost:8002/real_repo.git:/sub1
   + bdc926c...eb6a311 master     -> origin/master  (forced update)
  $ git fetch
  $ grep -o "fetch_cached_ok true" ${TESTTMP}/josh-proxy.out | uniq
  fetch_cached_ok true

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub1']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       `-- %3A%2Fsub1
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

# setup without caching
  $ EXTRA_OPTS= . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.
  $ curl -s http://localhost:8002/version
  Version: 22.4.15
  $ cd real_repo
  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir sub1
  $ echo content1 > sub1/file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master
  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git

# test without caching
  $ cd sub1
  $ echo "content2" > file2
  $ git add .
  $ git commit -m "add file 2"
  [master bdc926c] add file 2
   1 file changed, 1 insertion(+)
   create mode 100644 file2
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    5a29d3e..2e310c4  JOSH_PUSH -> master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
     eb6a311..bdc926c  master -> master
  $ git fetch
  $ git fetch
# no match
  $ grep -o "fetch_cached_ok true" ${TESTTMP}/josh-proxy.out | uniq

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub1']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       `-- %3A%2Fsub1
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
