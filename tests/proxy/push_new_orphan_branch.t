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
  $ git checkout --orphan orphan_branch 1> /dev/null
  Switched to a new branch 'orphan_branch'
  $ echo unrelated > orphan_file
  $ git add orphan_file
  $ git commit -m "orphan commit" 1> /dev/null
  $ git checkout master 1> /dev/null
  Switched to branch 'master'
  $ git merge --no-ff --allow-unrelated-histories -m "merge orphan" orphan_branch 1> /dev/null
  $ git push origin HEAD:refs/heads/new_branch 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 500 Internal Server Error
  remote: upstream: response body:
  remote:
  remote: Reference "refs/heads/new_branch" does not exist on remote.
  remote: If you want to create it, pass "-o base=<basebranch>" or "-o base=path/to/ref"
  remote: to specify a base branch/reference.
  remote:
  remote: error: hook declined to update refs/heads/new_branch
  To http://localhost:8002/real_repo.git:/sub1.git
   ! [remote rejected] HEAD -> new_branch (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:/sub1.git'

  $ git push -o base=refs/heads/master -o allow_orphans origin HEAD:refs/heads/new_branch 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new branch]      JOSH_PUSH -> new_branch
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      HEAD -> new_branch

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git push
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 500 Internal Server Error        
  remote: upstream: response body:        
  remote: 
  remote: Rejecting new orphan branch at "merge orphan" (b960d4fb2014cdabe5caa60b6e3bf8e3f1ee5a05)        
  remote: Specify one of these options:        
  remote:   '-o allow_orphans' to keep the history as is        
  remote:   '-o merge' to import new history by creating merge commit        
  remote:   '-o edit' if you are editing a stored filter or workspace        
  remote: 
  remote: error: hook declined to update refs/heads/master        
  To http://localhost:8002/real_repo.git:/sub1.git
   ! [remote rejected] master -> master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:/sub1.git'
  [1]

  $ git push -o allow_orphans
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    bb282e9..e61d37d  JOSH_PUSH -> master        
  To http://localhost:8002/real_repo.git:/sub1.git
     0b4cf6c..b960d4f  master -> master

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     bb282e9..e61d37d  master     -> origin/master
   * [new branch]      new_branch -> origin/new_branch
  Updating bb282e9..e61d37d
  Fast-forward
   sub1/file2       | 1 +
   sub1/orphan_file | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 sub1/file2
   create mode 100644 sub1/orphan_file

  $ git log --graph --pretty=%s
  *   merge orphan
  |\  
  | * orphan commit
  * add file2
  * add file1

  $ tree
  .
  `-- sub1
      |-- file1
      |-- file2
      `-- orphan_file
  
  2 directories, 3 files

  $ cat sub1/file2
  contents2

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
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 81
  |   |   |   `-- b10fb4984d20142cd275b89c91c346e536876a
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a4
  |   |   |   `-- ae8248b2e96725156258b90ced9e841dfd20d1
  |   |   |-- b1
  |   |   |   `-- d5238086b7f07024d8ed47360e3ce161d9b288
  |   |   |-- ba
  |   |   |   `-- 7e17233d9f79c96cb694959eb065302acd96a6
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c2
  |   |   |   `-- 1c9352f7526e9576892a6631e0e8cf1fccd34d
  |   |   |-- c6
  |   |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- d8
  |   |   |   `-- 43530e8283da7185faac160347db5c70ef4e18
  |   |   |-- e6
  |   |   |   `-- 1d37de15923090979cf667263aefa07f78cc33
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
  |       |                   |-- master
  |       |                   `-- new_branch
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
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 81
      |   |   `-- b10fb4984d20142cd275b89c91c346e536876a
      |   |-- a4
      |   |   `-- ae8248b2e96725156258b90ced9e841dfd20d1
      |   |-- b1
      |   |   `-- d5238086b7f07024d8ed47360e3ce161d9b288
      |   |-- b9
      |   |   `-- 60d4fb2014cdabe5caa60b6e3bf8e3f1ee5a05
      |   |-- ba
      |   |   `-- 7e17233d9f79c96cb694959eb065302acd96a6
      |   |-- c2
      |   |   `-- 1c9352f7526e9576892a6631e0e8cf1fccd34d
      |   |-- c6
      |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
      |   |-- d8
      |   |   |-- 388f5880393d255b371f1ed9b801d35620017e
      |   |   `-- 43530e8283da7185faac160347db5c70ef4e18
      |   |-- df
      |   |   `-- b06d7748772bdd407c5911c0ba02b0f5fb31a4
      |   |-- e6
      |   |   `-- 1d37de15923090979cf667263aefa07f78cc33
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  53 directories, 41 files

$ cat ${TESTTMP}/josh-proxy.out
