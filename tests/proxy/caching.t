# setup with caching
  $ EXTRA_OPTS=--cache-duration\ 100 . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.
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
  $ grep -o "cache ref resolved" ${TESTTMP}/josh-proxy.out | uniq
  cache ref resolved

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- 17
  |       `-- sled
  |           |-- blobs
  |           |-- conf
  |           `-- db
  |-- mirror
  |   |-- FETCH_HEAD
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- 5a
  |   |   |   `-- 29d3ef74b34ca534513c6499e9c9011371fab4
  |   |   |-- 8e
  |   |   |   `-- 9aa8ffc35bbc452f9654b834047168ce02dc48
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               `-- heads
  |       |                   `-- master
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 06
      |   |   `-- 8bef6976001fadcf131ff077eac37662ed3b7c
      |   |-- 2e
      |   |   `-- 310c493edb401f297aba1fc499506e4c85ca87
      |   |-- 63
      |   |   `-- 7f0347d31dad180d6fc7f6720c187b05a8754c
      |   |-- 77
      |   |   `-- d7000ec2f13c49605cd675075e1852881e4fea
      |   |-- bd
      |   |   `-- c926c483c4dfa77e84105c6967e74d8ff9bf5f
      |   |-- eb
      |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  37 directories, 23 files

# setup without caching
  $ EXTRA_OPTS= . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.
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
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- 17
  |       `-- sled
  |           |-- blobs
  |           |-- conf
  |           `-- db
  |-- mirror
  |   |-- FETCH_HEAD
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- 06
  |   |   |   `-- 8bef6976001fadcf131ff077eac37662ed3b7c
  |   |   |-- 2e
  |   |   |   `-- 310c493edb401f297aba1fc499506e4c85ca87
  |   |   |-- 5a
  |   |   |   `-- 29d3ef74b34ca534513c6499e9c9011371fab4
  |   |   |-- 63
  |   |   |   `-- 7f0347d31dad180d6fc7f6720c187b05a8754c
  |   |   |-- 77
  |   |   |   `-- d7000ec2f13c49605cd675075e1852881e4fea
  |   |   |-- 8e
  |   |   |   `-- 9aa8ffc35bbc452f9654b834047168ce02dc48
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               `-- heads
  |       |                   `-- master
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 06
      |   |   `-- 8bef6976001fadcf131ff077eac37662ed3b7c
      |   |-- 2e
      |   |   `-- 310c493edb401f297aba1fc499506e4c85ca87
      |   |-- 63
      |   |   `-- 7f0347d31dad180d6fc7f6720c187b05a8754c
      |   |-- 77
      |   |   `-- d7000ec2f13c49605cd675075e1852881e4fea
      |   |-- bd
      |   |   `-- c926c483c4dfa77e84105c6967e74d8ff9bf5f
      |   |-- eb
      |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  41 directories, 27 files
