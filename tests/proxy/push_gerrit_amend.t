  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ mkdir sub1
  $ mkdir sub2
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub2/file2
  $ git add .
  $ git commit -m "add files" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git sub1
  $ cd sub1

  $ echo contents1_new > file1
  $ git add file1
  $ git commit -m "Change-Id: 1234" 1> /dev/null
  $ git log --decorate --graph --pretty="%s %d"
  * Change-Id: 1234  (HEAD -> master)
  * add files  (origin/master, origin/HEAD)
  $ git push -o author=josh@example.com origin master:refs/for/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      1234 -> @changes/master/josh@example.com/1234        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      master -> @heads/master/josh@example.com        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin
  From http://localhost:8001/real_repo
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com
  $ git diff origin/@changes/master/josh@example.com/1234~1..origin/@changes/master/josh@example.com/1234
  diff --git a/sub1/file1 b/sub1/file1
  index a024003..70a938d 100644
  --- a/sub1/file1
  +++ b/sub1/file1
  @@ -1 +1 @@
  -contents1
  +contents1_new

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub2.git sub2
  $ cd sub2
  $ git log --decorate --graph --pretty="%s %d"
  * add files  (HEAD -> master, origin/master, origin/HEAD, origin/@heads/master/josh@example.com, origin/@changes/master/josh@example.com/1234)

  $ echo contents2_new > file2
  $ git add file2
  $ git commit -m "Change-Id: 1234" 1> /dev/null
  $ git push http://localhost:8001/real_repo.git :refs/for/master
  To http://localhost:8001/real_repo.git
   - [deleted]         refs/for/master
  $ git push -o author=josh@example.com origin master:refs/for/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: Everything up-to-date        
  remote: To http://localhost:8001/real_repo.git        
  remote:  + 9b69fe2...920a7be 1234 -> @changes/master/josh@example.com/1234 (forced update)        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master        
  remote: To http://localhost:8001/real_repo.git        
  remote:  + 9b69fe2...920a7be master -> @heads/master/josh@example.com (forced update)        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub2.git
   * [new reference]   master -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin
  From http://localhost:8001/real_repo
   + 9b69fe2...920a7be @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234  (forced update)
   + 9b69fe2...920a7be @heads/master/josh@example.com -> origin/@heads/master/josh@example.com  (forced update)
  $ git diff origin/@changes/master/josh@example.com/1234~1..origin/@changes/master/josh@example.com/1234
  diff --git a/sub1/file1 b/sub1/file1
  index a024003..70a938d 100644
  --- a/sub1/file1
  +++ b/sub1/file1
  @@ -1 +1 @@
  -contents1
  +contents1_new
  diff --git a/sub2/file2 b/sub2/file2
  index 6b46faa..72e4684 100644
  --- a/sub2/file2
  +++ b/sub2/file2
  @@ -1 +1 @@
  -contents2
  +contents2_new

  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git fetch origin

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1",
      ":/sub2",
      "::sub1/",
      "::sub2/",
  ]
  .
  |-- josh
  |   `-- 18
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
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 63
  |   |   |   `-- 7d3debfcb3bdadc2ebd92e6451bcb34aebcec1
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 70
  |   |   |   `-- a938dff577e016189da58f38b71cf0ab3d4cbb
  |   |   |-- 95
  |   |   |   `-- 3f19a771cbc2937546fec3b0b155fd2ffe26be
  |   |   |-- 9b
  |   |   |   |-- 13ff5c153ca4bc05cb337ad4dfcfe4ffe7af46
  |   |   |   `-- 69fe244c5e0cbc08d2aba426f1ce639ab4e017
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- ae
  |   |   |   `-- a557394ce29f000108607abd97f19fed4d1b7c
  |   |   |-- ca
  |   |   |   `-- 77fb80b683ebe1fd4d4d6c2dee5d247f9befee
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
  |       |                   |-- @changes
  |       |                   |   `-- master
  |       |                   |       `-- josh@example.com
  |       |                   |           `-- 1234
  |       |                   |-- @heads
  |       |                   |   `-- master
  |       |                   |       `-- josh@example.com
  |       |                   `-- master
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 19
      |   |   `-- 4865382d22d1f0b6bc65029c74833d4509d6cd
      |   |-- 25
      |   |   `-- f54a31709f8520b0798c743ee1efcbe47069ce
      |   |-- 29
      |   |   `-- 7ce4243f24c3ce36b47ed90b1e6b788937c633
      |   |-- 31
      |   |   `-- 8ff73ef0235d761f6aee1194a6bdaaeb1d0923
      |   |-- 52
      |   |   `-- be19f48566b18ccf49846b221d84f0b75cae66
      |   |-- 63
      |   |   `-- 7d3debfcb3bdadc2ebd92e6451bcb34aebcec1
      |   |-- 6c
      |   |   `-- 94de87e1c04cbb8292abe90fc419c51113cfc2
      |   |-- 70
      |   |   `-- a938dff577e016189da58f38b71cf0ab3d4cbb
      |   |-- 72
      |   |   `-- e468401514da24f311bbc90908bff97b3f503a
      |   |-- 78
      |   |   `-- 94b7b9ac2a655147a5c0935002ea977b99d83d
      |   |-- 92
      |   |   `-- 0a7beb2c682ae84bdb9cd786a1bdd38891bced
      |   |-- 9b
      |   |   |-- 13ff5c153ca4bc05cb337ad4dfcfe4ffe7af46
      |   |   `-- 69fe244c5e0cbc08d2aba426f1ce639ab4e017
      |   |-- a7
      |   |   `-- 8fae76b871ddf5473364d136f384d9613df3e7
      |   |-- b6
      |   |   `-- 44f072c8707dc0a051ae120591d312973787dc
      |   |-- c8
      |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  56 directories, 41 files

$ cat ${TESTTMP}/josh-proxy.out
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
