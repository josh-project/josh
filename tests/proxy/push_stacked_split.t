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
  $ git commit -m "Change: 1234" 1> /dev/null
  $ echo contents2 > file7
  $ git add file7
  $ git commit -m "Change: foo7" 1> /dev/null
  $ echo contents3 > file2
  $ git add file2
  $ git commit -m "Change: 1235" 1> /dev/null
  $ git log --decorate --graph --pretty="%s %d"
  * Change: 1235  (HEAD -> master)
  * Change: foo7 
  * Change: 1234 
  * add file1  (origin/master, origin/HEAD)
  $ git push -o split -o author=josh@example.com origin master:refs/split/for/master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      1234 -> @changes/master/josh@example.com/1234        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      foo7 -> @changes/master/josh@example.com/foo7        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      1235 -> @changes/master/josh@example.com/1235        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      master -> @heads/master/josh@example.com        
  remote: 
  remote: 
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
  * Change: 1235  (HEAD -> master, origin/@heads/master/josh@example.com)
  * Change: foo7 
  | * Change: 1235  (origin/@changes/master/josh@example.com/1235)
  |/  
  * Change: 1234  (origin/@changes/master/josh@example.com/1234)
  | * Change: foo7  (origin/@changes/master/josh@example.com/foo7)
  |/  
  * add file1  (origin/master, origin/HEAD)

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub1']
  .
  |-- josh
  |   `-- 13
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
  |   |   |-- 0e
  |   |   |   `-- 7970a3dec1b5bbba1dce0450d7a7dfc56a9797
  |   |   |-- 14
  |   |   |   `-- 20ebc1972205ad0fe84c31eec84a8a7a334882
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 36
  |   |   |   `-- c2216dd9dc554e35d4ae37794eb72392ae2591
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 49
  |   |   |   |-- 03654ec80c5cff86ab37a0b9d7bcf8332e8c54
  |   |   |   `-- 50fa502f51b7bfda0d7975dbff9b0f9a9481ca
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 72
  |   |   |   `-- 7902c81346e29c4f75e0913bd62d7b85d7033f
  |   |   |-- 80
  |   |   |   `-- b41c2bfde183168405cb8707032a8150184f6f
  |   |   |-- 82
  |   |   |   `-- 599da2054669a020103a7bd8aa456540a0c5ee
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
  |   |   |-- ed
  |   |   |   `-- b2a5b9c65fae1d20c1b1fb777d1ea025456faa
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
      |   |-- 0e
      |   |   `-- 7970a3dec1b5bbba1dce0450d7a7dfc56a9797
      |   |-- 14
      |   |   `-- 20ebc1972205ad0fe84c31eec84a8a7a334882
      |   |-- 1c
      |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
      |   |-- 36
      |   |   `-- c2216dd9dc554e35d4ae37794eb72392ae2591
      |   |-- 49
      |   |   `-- 03654ec80c5cff86ab37a0b9d7bcf8332e8c54
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 72
      |   |   `-- 7902c81346e29c4f75e0913bd62d7b85d7033f
      |   |-- 80
      |   |   `-- b41c2bfde183168405cb8707032a8150184f6f
      |   |-- 82
      |   |   `-- 599da2054669a020103a7bd8aa456540a0c5ee
      |   |-- 8a
      |   |   `-- 778416d88308bf017cf54f0247e4780765361f
      |   |-- b2
      |   |   `-- dd517c55420a48cb543e0195b4751bf514b941
      |   |-- ed
      |   |   `-- b2a5b9c65fae1d20c1b1fb777d1ea025456faa
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  59 directories, 46 files

$ cat ${TESTTMP}/josh-proxy.out
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
