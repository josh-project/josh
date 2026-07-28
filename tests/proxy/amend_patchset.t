  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ echo content1 > file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file3
  * initial

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git full
  $ cd ${TESTTMP}/full
  $ tree
  .
  |-- file1
  `-- sub3
      `-- file3
  
  2 directories, 2 files

  $ git log --graph --pretty=%s
  * add file3
  * initial

  $ echo content2 > file_outside 1> /dev/null
  $ echo content3 > sub3/file2x 1> /dev/null
  $ git add .
  $ git commit -aq -F - <<EOF
  > Add in full
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  To http://localhost:8002/real_repo.git
   * [new reference]   HEAD -> refs/for/master

  $ cd ${TESTTMP}/remote/real_repo.git/
  $ git update-ref refs/changes/1/1 refs/for/master
  $ git update-ref -d refs/for/master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub3.git sub
  $ cd ${TESTTMP}/sub
  $ git fetch -q http://localhost:8002/real_repo.git@refs/changes/1/1:/sub3.git && git checkout -q FETCH_HEAD
  $ git log --graph --pretty=%s
  * Add in full
  * add file3
  $ tree
  .
  |-- file2x
  `-- file3
  
  1 directory, 2 files

  $ echo content4 > file_new 1> /dev/null
  $ git add .
  $ git commit --amend --no-edit -q
  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  To http://localhost:8002/real_repo.git:/sub3.git
   * [new reference]   HEAD -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch -q http://localhost:8002/real_repo.git@refs/for/master:nop.git && git checkout -q FETCH_HEAD
  $ git log --graph --pretty=%s
  * Add in full
  * add file3
  * initial
  $ tree
  .
  |-- file1
  `-- sub3
      |-- file2x
      |-- file3
      `-- file_new
  
  2 directories, 4 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub3",
      "::sub3/",
  ]
  .
  |-- josh
  |   `-- cache
  |       `-- 32
  |           `-- sled
  |               |-- blobs
  |               |-- conf
  |               `-- db
  |-- mirror
  |   |-- FETCH_HEAD
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- 18
  |   |   |   `-- 5984b1d05c7ba7842828bdc8669c69eed48540
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 22
  |   |   |   `-- a3c9e53508f9532b5109352448c0051ff0a018
  |   |   |-- 2a
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 50
  |   |   |   `-- e9d1a4ad68f5edd03432b540ae6d56995810f5
  |   |   |-- 63
  |   |   |   `-- 9874a1a8b362d3042d6bc74339166a13fa78b3
  |   |   |-- 76
  |   |   |   `-- b2b18cc389f8dc88727cb143f362f3b4a07788
  |   |   |-- 8e
  |   |   |   `-- 9eedb14562d157b873eb24a08d6c0cd225624b
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- b0
  |   |   |   `-- c372112e15e6946f82bebf73b70f5b3e0d5066
  |   |   |-- e5
  |   |   |   `-- 70b660ac9df7044f7262287a3828b44bb001b3
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- eb
  |   |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               |-- changes
  |       |               |   `-- 1
  |       |               |       `-- 1
  |       |               |-- for
  |       |               |   `-- master
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
      |   |-- 18
      |   |   `-- 5984b1d05c7ba7842828bdc8669c69eed48540
      |   |-- 22
      |   |   `-- a3c9e53508f9532b5109352448c0051ff0a018
      |   |-- 50
      |   |   `-- e9d1a4ad68f5edd03432b540ae6d56995810f5
      |   |-- 8e
      |   |   `-- 9eedb14562d157b873eb24a08d6c0cd225624b
      |   |-- c3
      |   |   `-- 4ad756a74e200f89b43b0d6f21b41eb284b454
      |   |-- info
      |   `-- pack
      |       |-- pack-9f1d5b705f285451a89d8e75d2e3ed95ba8cc998.idx
      |       |-- pack-9f1d5b705f285451a89d8e75d2e3ed95ba8cc998.pack
      |       |-- pack-a0d4d7da863ed9bcde8d55093ccc35b5c0e7fd27.idx
      |       |-- pack-a0d4d7da863ed9bcde8d55093ccc35b5c0e7fd27.pack
      |       |-- pack-b1bafc32ba8e81e4a1a2f5a5d465d9bc1eab7ee1.idx
      |       `-- pack-b1bafc32ba8e81e4a1a2f5a5d465d9bc1eab7ee1.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  49 directories, 39 files
