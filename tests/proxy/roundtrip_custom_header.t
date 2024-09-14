  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ git log --oneline --graph
  * bb282e9 add file1

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:prefix=pre.git pre
  $ cd pre

  $ echo contents2 > pre/file2
  $ git add .
  $ git commit -m "add file2" &> /dev/null
  $ git log --oneline --graph
  * f99a2dd add file2
  * 1f0b9d8 add file1

Write a custom header into the commit (h/t https://github.com/Byron/gitoxide/blob/68cbea8gix/tests/fixtures/make_pre_epoch_repo.sh#L12-L27)
  $ git cat-file -p @ | tee commit.txt
  tree 554b59f0a39f6968f0101b8a0471f8a65bc25020
  parent 1f0b9d8a7b40a35bb4ff64ffc0f08369df23bc61
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  
  add file2
  $ patch -p1 <<EOF
  > diff --git a/commit.txt b/commit.txt
  > index 1758866..fe1998a 100644
  > --- a/commit.txt
  > +++ b/commit.txt
  > @@ -2,5 +2,7 @@ tree 554b59f0a39f6968f0101b8a0471f8a65bc25020
  >  parent 1f0b9d8a7b40a35bb4ff64ffc0f08369df23bc61
  >  author Josh <josh@example.com> 1112911993 +0000
  >  committer Josh <josh@example.com> 1112911993 +0000
  > +custom-header and value
  > +another-header such that it sorts before custom-header
  >  
  >  add file2
  > EOF
  patching file commit.txt
  $ new_commit=$(git hash-object --literally -w -t commit commit.txt)
  $ git update-ref refs/heads/master $new_commit
  $ git log --oneline --graph
  * c74f96c add file2
  * 1f0b9d8 add file1
  $ git push
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    bb282e9..edf5151  JOSH_PUSH -> master        
  To http://localhost:8002/real_repo.git:prefix=pre.git
     1f0b9d8..c74f96c  master -> master

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     bb282e9..edf5151  master     -> origin/master
  Updating bb282e9..edf5151
  Fast-forward
   file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 file2

  $ tree
  .
  |-- file2
  `-- sub1
      `-- file1
  
  2 directories, 2 files

  $ git log --oneline --graph
  * edf5151 add file2
  * bb282e9 add file1

Re-clone to verify that the rewritten commit c74f96c is restored and the custom headers are preserved
  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:prefix=pre.git pre2
  $ cd pre2
  $ git log --oneline --graph
  * c74f96c add file2
  * 1f0b9d8 add file1
  $ git cat-file -p @
  tree 554b59f0a39f6968f0101b8a0471f8a65bc25020
  parent 1f0b9d8a7b40a35bb4ff64ffc0f08369df23bc61
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  custom-header and value
  another-header such that it sorts before custom-header
  
  add file2

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub1/",
      ":prefix=pre",
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
  |   |   |-- 0f
  |   |   |   `-- 17ab2c89a1278ecb6a7438e915e491884d3efb
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- ed
  |   |   |   `-- f51518ffef5a69791a6e38a6656068aeb2cf8e
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
      |   |-- 0f
      |   |   `-- 17ab2c89a1278ecb6a7438e915e491884d3efb
      |   |-- 1f
      |   |   `-- 0b9d8a7b40a35bb4ff64ffc0f08369df23bc61
      |   |-- 55
      |   |   `-- 4b59f0a39f6968f0101b8a0471f8a65bc25020
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- b5
      |   |   `-- af4d1258141efaadc32e369f4dc4b1f6c524e4
      |   |-- c7
      |   |   `-- 4f96c02a8f34cd4321d646e728aeb11ea34932
      |   |-- ed
      |   |   `-- f51518ffef5a69791a6e38a6656068aeb2cf8e
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  41 directories, 27 files
