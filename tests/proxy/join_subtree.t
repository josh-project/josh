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
  $ git show
  commit d8388f5880393d255b371f1ed9b801d35620017e
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file2
  
  diff --git a/file2 b/file2
  new file mode 100644
  index 0000000..6b46faa
  --- /dev/null
  +++ b/file2
  @@ -0,0 +1 @@
  +contents2
  $ git push http://localhost:8001/real/repo2.git HEAD:refs/heads/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  To http://localhost:8001/real/repo2.git
   * [new branch]      HEAD -> master

  $ git fetch "http://localhost:8002/real_repo.git@refs/heads/master:join('/real/repo2.git@refs/heads/master':/sub1).git"
  From http://localhost:8002/real_repo.git@refs/heads/master:join('/real/repo2.git@refs/heads/master':/sub1)
   * branch            HEAD       -> FETCH_HEAD

  $ git fetch "http://localhost:8002/real_repo.git@refs/heads/master:join(%22/real/repo2.git@refs/heads/master%22:/sub1).git"
  From http://localhost:8002/real_repo.git@refs/heads/master:join(%22/real/repo2.git@refs/heads/master%22:/sub1)
   * branch            HEAD       -> FETCH_HEAD
  $ git log --graph --pretty=%s-%H FETCH_HEAD
  * add file2-81b10fb4984d20142cd275b89c91c346e536876a
  * add file1-bb282e9cdc1b972fffd08fd21eead43bc0c83cb8
  $ git fetch "http://localhost:8002/real_repo.git@refs/heads/master:join(d8388f5880393d255b371f1ed9b801d35620017e:/sub1).git"
  From http://localhost:8002/real_repo.git@refs/heads/master:join(d8388f5880393d255b371f1ed9b801d35620017e:/sub1)
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s-%H FETCH_HEAD
  * add file2-81b10fb4984d20142cd275b89c91c346e536876a
  * add file1-bb282e9cdc1b972fffd08fd21eead43bc0c83cb8

  $ git diff ${EMPTY_TREE}..FETCH_HEAD
  diff --git a/sub1/file1 b/sub1/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub1/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/sub1/file2 b/sub1/file2
  new file mode 100644
  index 0000000..6b46faa
  --- /dev/null
  +++ b/sub1/file2
  @@ -0,0 +1 @@
  +contents2


  $ git push -o base=refs/heads/master origin HEAD:refs/heads/new_branch 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
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
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    bb282e9..81b10fb  JOSH_PUSH -> master        
  To http://localhost:8002/real_repo.git:/sub1.git
     0b4cf6c..d8388f5  master -> master

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     bb282e9..81b10fb  master     -> origin/master
   * [new branch]      new_branch -> origin/new_branch
  Updating bb282e9..81b10fb
  Fast-forward
   sub1/file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  2 directories, 2 files

  $ cat sub1/file2
  contents2

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real/repo2.git" = []
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
      ':join("/real/repo2.git@refs/heads/master":/sub1)',
      ":join(d8388f5880393d255b371f1ed9b801d35620017e:/sub1)",
  ]
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
  |   |   |-- 0b
  |   |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 81
  |   |   |   `-- b10fb4984d20142cd275b89c91c346e536876a
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- ba
  |   |   |   `-- 7e17233d9f79c96cb694959eb065302acd96a6
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c6
  |   |   |   `-- 27a2e3a6bfbb7307f522ad94fdfc8c20b92967
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- d8
  |   |   |   `-- 388f5880393d255b371f1ed9b801d35620017e
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       |-- real%2Frepo2.git
  |       |       |   |-- HEAD
  |       |       |   `-- refs
  |       |       |       `-- heads
  |       |       |           `-- master
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
      |   |-- 81
      |   |   `-- b10fb4984d20142cd275b89c91c346e536876a
      |   |-- ba
      |   |   `-- 7e17233d9f79c96cb694959eb065302acd96a6
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  43 directories, 29 files

$ cat ${TESTTMP}/josh-proxy.out
