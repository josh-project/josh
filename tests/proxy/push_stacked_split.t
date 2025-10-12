  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo before > file7
  $ git add .
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git sub1
  $ cd sub1

  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "Change-Id: 1234" 1> /dev/null
  $ echo contents2 > file7
  $ git add file7
  $ git commit -m "Change-Id: foo7" 1> /dev/null
  $ echo contents3 > file2
  $ git add file2
  $ git commit -m "Change-Id: 1235" 1> /dev/null
  $ git log --decorate --graph --pretty="%s %d"
  * Change-Id: 1235  (HEAD -> master)
  * Change-Id: foo7 
  * Change-Id: 1234 
  * add file1  (origin/master, origin/HEAD)
  $ git push -o split -o author=josh@example.com origin master:refs/split/for/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      1234 -> @changes/master/josh@example.com/1234        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      foo7 -> @changes/master/josh@example.com/foo7        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      1235 -> @changes/master/josh@example.com/1235        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      master -> @heads/master/josh@example.com        
  To http://localhost:8002/real_repo.git
   * [new reference]   master -> refs/split/for/master

  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git fetch origin
  From http://localhost:8002/real_repo
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/1235 -> origin/@changes/master/josh@example.com/1235
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com

  $ git log --all --decorate --graph --pretty="%s %d"
  * Change-Id: 1235  (HEAD -> master, origin/@heads/master/josh@example.com)
  * Change-Id: foo7 
  | * Change-Id: 1235  (origin/@changes/master/josh@example.com/1235)
  |/  
  * Change-Id: 1234  (origin/@changes/master/josh@example.com/1234)
  | * Change-Id: foo7  (origin/@changes/master/josh@example.com/foo7)
  |/  
  * add file1  (origin/master, origin/HEAD)

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = ["::sub1/"]
  .
  |-- josh
  |   `-- 23
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
  |   |   |-- 14
  |   |   |   `-- 20ebc1972205ad0fe84c31eec84a8a7a334882
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 3b
  |   |   |   `-- 0e3dbefd779ec54d92286047f32d3129161c0d
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 49
  |   |   |   `-- 50fa502f51b7bfda0d7975dbff9b0f9a9481ca
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 72
  |   |   |   `-- 7902c81346e29c4f75e0913bd62d7b85d7033f
  |   |   |-- 7e
  |   |   |   `-- 693682dba1d7af1cb1eca7c3dcc5128e3ec9c6
  |   |   |-- 85
  |   |   |   `-- 90a3b0b3086ab857b91581c320e377dc9780ea
  |   |   |-- 8a
  |   |   |   `-- 778416d88308bf017cf54f0247e4780765361f
  |   |   |-- 90
  |   |   |   `-- be1f3056c4f471f977a28497b8d4b392c55a02
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- b2
  |   |   |   `-- dd517c55420a48cb543e0195b4751bf514b941
  |   |   |-- b7
  |   |   |   `-- 8fc0fe9f3bd35c8dc8aeff5189ebda0750e974
  |   |   |-- ec
  |   |   |   `-- 41aad70b4b898baf48efeb795a7753d9674152
  |   |   |-- ed
  |   |   |   `-- b2a5b9c65fae1d20c1b1fb777d1ea025456faa
  |   |   |-- fd
  |   |   |   `-- b894f9431a237930c7a034f3ecc16dd686b19e
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
  |       |                   |           |-- 1234
  |       |                   |           |-- 1235
  |       |                   |           `-- foo7
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
      |   |-- 14
      |   |   `-- 20ebc1972205ad0fe84c31eec84a8a7a334882
      |   |-- 1c
      |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
      |   |-- 3b
      |   |   `-- 0e3dbefd779ec54d92286047f32d3129161c0d
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 72
      |   |   `-- 7902c81346e29c4f75e0913bd62d7b85d7033f
      |   |-- 7e
      |   |   `-- 693682dba1d7af1cb1eca7c3dcc5128e3ec9c6
      |   |-- 8a
      |   |   `-- 778416d88308bf017cf54f0247e4780765361f
      |   |-- b2
      |   |   `-- dd517c55420a48cb543e0195b4751bf514b941
      |   |-- b7
      |   |   `-- 8fc0fe9f3bd35c8dc8aeff5189ebda0750e974
      |   |-- ec
      |   |   `-- 41aad70b4b898baf48efeb795a7753d9674152
      |   |-- ed
      |   |   `-- b2a5b9c65fae1d20c1b1fb777d1ea025456faa
      |   |-- fd
      |   |   `-- b894f9431a237930c7a034f3ecc16dd686b19e
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  61 directories, 46 files

$ cat ${TESTTMP}/josh-proxy.out
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
