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
  $ git commit -m "Change-Id: 1234" 1> /dev/null
  $ echo contents2 > file7
  $ git add file7
  $ git commit -m "Change-Id: foo7" 1> /dev/null
  $ git log --decorate --graph --pretty="%s %d"
  * Change-Id: foo7  (HEAD -> master)
  * Change-Id: 1234 
  * add file1  (origin/master, origin/HEAD)
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
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/stack/for/master
  $ git push -o author=josh@example.com origin master:refs/stack/for/other_branch
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 500 Internal Server Error        
  remote: upstream: response body:        
  remote: 
  remote: Reference "refs/heads/other_branch" does not exist on remote.        
  remote: If you want to create it, pass "-o base=<basebranch>" or "-o base=path/to/ref"        
  remote: to specify a base branch/reference.        
  remote: 
  remote: error: hook declined to update refs/stack/for/other_branch        
  To http://localhost:8002/real_repo.git:/sub1.git
   ! [remote rejected] master -> refs/stack/for/other_branch (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:/sub1.git'
  [1]
  $ git push -o base=master -o author=josh@example.com origin master:refs/stack/for/other_branch
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      1234 -> @changes/other_branch/josh@example.com/1234        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      foo7 -> @changes/other_branch/josh@example.com/foo7        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      other_branch -> @heads/other_branch/josh@example.com        
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/stack/for/other_branch
  $ echo contents2 > file3
  $ git add file3
  $ git commit -m "add file3" 1> /dev/null
  $ git push -o author=josh@example.com origin master:refs/stack/for/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: Everything up-to-date        
  remote: Everything up-to-date        
  remote: To http://localhost:8001/real_repo.git        
  remote:    2bb9471..a306516  master -> @heads/master/josh@example.com        
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new reference]   master -> refs/stack/for/master

  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git fetch origin
  From http://localhost:8002/real_repo.git:/sub1
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @changes/other_branch/josh@example.com/1234 -> origin/@changes/other_branch/josh@example.com/1234
   * [new branch]      @changes/other_branch/josh@example.com/foo7 -> origin/@changes/other_branch/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com
   * [new branch]      @heads/other_branch/josh@example.com -> origin/@heads/other_branch/josh@example.com
  $ git log --decorate --graph --pretty="%s %d"
  * add file3  (HEAD -> master, origin/@heads/master/josh@example.com)
  * Change-Id: foo7  (origin/@heads/other_branch/josh@example.com, origin/@changes/other_branch/josh@example.com/foo7, origin/@changes/master/josh@example.com/foo7)
  * Change-Id: 1234  (origin/@changes/other_branch/josh@example.com/1234, origin/@changes/master/josh@example.com/1234)
  * add file1  (origin/master, origin/HEAD)

  $ cd ${TESTTMP}/real_repo
  $ git fetch origin
  From http://localhost:8001/real_repo
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @changes/other_branch/josh@example.com/1234 -> origin/@changes/other_branch/josh@example.com/1234
   * [new branch]      @changes/other_branch/josh@example.com/foo7 -> origin/@changes/other_branch/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com
   * [new branch]      @heads/other_branch/josh@example.com -> origin/@heads/other_branch/josh@example.com
  $ git checkout -q heads/master/josh@example.com
  error: pathspec 'heads/master/josh@example.com' did not match any file(s) known to git
  [1]
  $ git log --decorate --graph --pretty="%s %d"
  * add file1  (HEAD -> master, origin/master, origin/HEAD)

  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- 24
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
  |   |   |-- 2b
  |   |   |   `-- b94719cad2e76eb5dbcae874a2b0e44521e802
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 88
  |   |   |   `-- 2b84c5d3241087bc41982a744b72b7a174c49e
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a3
  |   |   |   `-- 065162ecee0fecc977ec04a275e10b5e15a39c
  |   |   |-- b2
  |   |   |   `-- ea883bc5df63565960a38cad7a57f73ac66eaa
  |   |   |-- ba
  |   |   |   |-- 7e17233d9f79c96cb694959eb065302acd96a6
  |   |   |   `-- c8af20b53d712874a32944874c66a21afa91f9
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- be
  |   |   |   `-- 33ab805ad4ef7ddda5b51e4a78ec0fac6b699a
  |   |   |-- c6
  |   |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- e3
  |   |   |   `-- a269743538338b0d059eda2f72976ec29220a8
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
  |       |                   |   |-- master
  |       |                   |   |   `-- josh@example.com
  |       |                   |   |       |-- 1234
  |       |                   |   |       `-- foo7
  |       |                   |   `-- other_branch
  |       |                   |       `-- josh@example.com
  |       |                   |           |-- 1234
  |       |                   |           `-- foo7
  |       |                   |-- @heads
  |       |                   |   |-- master
  |       |                   |   |   `-- josh@example.com
  |       |                   |   `-- other_branch
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
      |   |-- 0b
      |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
      |   |-- 11
      |   |   `-- 06a6f09b169109f5ebd17428219142c67e84b9
      |   |-- 2b
      |   |   `-- b94719cad2e76eb5dbcae874a2b0e44521e802
      |   |-- 40
      |   |   `-- 3e13a101fd75090369ffc317afafcfb646c576
      |   |-- 4b
      |   |   `-- 5ffb79a1f3852443bd747a6b213bf1e4abe168
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 88
      |   |   `-- 2b84c5d3241087bc41982a744b72b7a174c49e
      |   |-- a3
      |   |   `-- 065162ecee0fecc977ec04a275e10b5e15a39c
      |   |-- b2
      |   |   `-- ea883bc5df63565960a38cad7a57f73ac66eaa
      |   |-- ba
      |   |   |-- 7e17233d9f79c96cb694959eb065302acd96a6
      |   |   `-- c8af20b53d712874a32944874c66a21afa91f9
      |   |-- be
      |   |   `-- 33ab805ad4ef7ddda5b51e4a78ec0fac6b699a
      |   |-- c6
      |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
      |   |-- e3
      |   |   `-- a269743538338b0d059eda2f72976ec29220a8
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  61 directories, 47 files

$ cat ${TESTTMP}/josh-proxy.out
$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
$ cat ${TESTTMP}/josh-proxy.out | grep "==="
