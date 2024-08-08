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
  $ git log --decorate --graph --pretty="%s %d"
  * Change-Id: foo7  (HEAD -> master)
  * Change-Id: 1234 
  * add file1  (origin/master, origin/HEAD)
  $ git push -o author=foo@example.com origin master:refs/stack/for/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      master -> @heads/master/foo@example.com        
  To http://localhost:8002/real_repo.git
   * [new reference]   master -> refs/stack/for/master
  $ git push -o author=josh@example.com origin master:refs/stack/for/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      1234 -> @changes/master/josh@example.com/1234        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      foo7 -> @changes/master/josh@example.com/foo7        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      master -> @heads/master/josh@example.com        
  To http://localhost:8002/real_repo.git
   * [new reference]   master -> refs/stack/for/master
  $ echo contents2 > file3
  $ git add file3
  $ git commit -m "add file3" 1> /dev/null
  $ git push -o author=josh@example.com origin master:refs/stack/for/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 500 Internal Server Error        
  remote: upstream: response body:        
  remote: 
  remote: rejecting to push 3ad32b3bd3bb778441e7eae43930d8dc6293eddc without id        
  remote: error: hook declined to update refs/stack/for/master        
  To http://localhost:8002/real_repo.git
   ! [remote rejected] master -> refs/stack/for/master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git'
  [1]
  $ git push -o author=foo@example.com origin master:refs/stack/for/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    ec41aad..3ad32b3  master -> @heads/master/foo@example.com        
  To http://localhost:8002/real_repo.git
   * [new reference]   master -> refs/stack/for/master

  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git fetch origin
  From http://localhost:8002/real_repo
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/foo@example.com -> origin/@heads/master/foo@example.com
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com

  $ git log --decorate --graph --pretty="%s %d"
  * add file3  (HEAD -> master, origin/@heads/master/foo@example.com)
  * Change-Id: foo7  (origin/@heads/master/josh@example.com, origin/@changes/master/josh@example.com/foo7)
  * Change-Id: 1234  (origin/@changes/master/josh@example.com/1234)
  * add file1  (origin/master, origin/HEAD)

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin
  From http://localhost:8001/real_repo
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/foo@example.com -> origin/@heads/master/foo@example.com
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com
  $ git checkout -q heads/master/foo@example.com
  error: pathspec 'heads/master/foo@example.com' did not match any file(s) known to git
  [1]
  $ git log --decorate --graph --pretty="%s %d"
  * add file1  (HEAD -> master, origin/master)

  $ tree
  .
  |-- file7
  `-- sub1
      `-- file1
  
  2 directories, 2 files

To avoid stacked changes to cause excessive amounts of refs, refs get filtered to only
get listed if they differ from HEAD

  $ git ls-remote http://localhost:8002/real_repo.git
  4950fa502f51b7bfda0d7975dbff9b0f9a9481ca\tHEAD (esc)
  3b0e3dbefd779ec54d92286047f32d3129161c0d\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  ec41aad70b4b898baf48efeb795a7753d9674152\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  3ad32b3bd3bb778441e7eae43930d8dc6293eddc\trefs/heads/@heads/master/foo@example.com (esc)
  ec41aad70b4b898baf48efeb795a7753d9674152\trefs/heads/@heads/master/josh@example.com (esc)
  4950fa502f51b7bfda0d7975dbff9b0f9a9481ca\trefs/heads/master (esc)

  $ git ls-remote http://localhost:8002/real_repo.git:/sub1.git
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\tHEAD (esc)
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\trefs/heads/@heads/master/foo@example.com (esc)
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\trefs/heads/@heads/master/josh@example.com (esc)
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb\trefs/heads/master (esc)
  $ git ls-remote http://localhost:8002/real_repo.git::file2.git
  $ git ls-remote http://localhost:8002/real_repo.git::file7.git
  23b2396b6521abcd906f16d8492c5aeacaee06ed\tHEAD (esc)
  23b2396b6521abcd906f16d8492c5aeacaee06ed\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  08c82a20e92d548ff32f86d634b82da6756e1f5f\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  08c82a20e92d548ff32f86d634b82da6756e1f5f\trefs/heads/@heads/master/foo@example.com (esc)
  08c82a20e92d548ff32f86d634b82da6756e1f5f\trefs/heads/@heads/master/josh@example.com (esc)
  23b2396b6521abcd906f16d8492c5aeacaee06ed\trefs/heads/master (esc)

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1",
      "::file2",
      "::file7",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- 22
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
  |   |   |-- 3a
  |   |   |   `-- d32b3bd3bb778441e7eae43930d8dc6293eddc
  |   |   |-- 3b
  |   |   |   `-- 0e3dbefd779ec54d92286047f32d3129161c0d
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 49
  |   |   |   `-- 50fa502f51b7bfda0d7975dbff9b0f9a9481ca
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 85
  |   |   |   `-- 90a3b0b3086ab857b91581c320e377dc9780ea
  |   |   |-- 90
  |   |   |   `-- be1f3056c4f471f977a28497b8d4b392c55a02
  |   |   |-- 9a
  |   |   |   `-- 91b9f3056d29fafb535b6e801f26449b291daf
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- b2
  |   |   |   `-- dd517c55420a48cb543e0195b4751bf514b941
  |   |   |-- ec
  |   |   |   `-- 41aad70b4b898baf48efeb795a7753d9674152
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
  |       |                   |           `-- foo7
  |       |                   |-- @heads
  |       |                   |   `-- master
  |       |                   |       |-- foo@example.com
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
      |   |-- 08
      |   |   `-- c82a20e92d548ff32f86d634b82da6756e1f5f
      |   |-- 0b
      |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
      |   |-- 0f
      |   |   `-- 8d6bce4783eb0366cc72cb5f6fb7952c360d89
      |   |-- 23
      |   |   `-- b2396b6521abcd906f16d8492c5aeacaee06ed
      |   |-- 38
      |   |   `-- 25d17d9f46345e352ea6d1d0a89f2be16a1b2c
      |   |-- 3a
      |   |   `-- d32b3bd3bb778441e7eae43930d8dc6293eddc
      |   |-- 3b
      |   |   `-- 0e3dbefd779ec54d92286047f32d3129161c0d
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 96
      |   |   `-- 539577d449c8e5446b5339c20436a13ec51f41
      |   |-- 9a
      |   |   `-- 91b9f3056d29fafb535b6e801f26449b291daf
      |   |-- ae
      |   |   `-- a557394ce29f000108607abd97f19fed4d1b7c
      |   |-- b2
      |   |   `-- dd517c55420a48cb543e0195b4751bf514b941
      |   |-- ec
      |   |   `-- 41aad70b4b898baf48efeb795a7753d9674152
      |   |-- ed
      |   |   `-- b2a5b9c65fae1d20c1b1fb777d1ea025456faa
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  59 directories, 44 files

$ cat ${TESTTMP}/josh-proxy.out
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
