This is a bit of a special case:
Normally when selecting commits to be included in the rewritten history Josh excludes
all commits whose tree has no diff to the parent.
This makes sure that when extracting subdirectories for example, only commits that affect
the directory of interest are included in the history.
However in case the diff was also empty in the untransformed commit this means the commit
was created on purpose (by passing --allow-empty to git commit) and in this case the commit
should still be included.

  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) bb282e9] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ git commit --allow-empty -m "x"
  [master 6ce3416] x

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2"
  [master df06f78] add file2
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  2 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * x
  * add file1

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git full_repo

  $ cd full_repo

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  2 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * x
  * add file1

  $ cat sub1/file1
  contents1

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = ["::sub1/"]
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
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 6c
  |   |   |   `-- e34162a27f0c0cd1072a5f42cc0424dec4d1be
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
  |   |   |-- df
  |   |   |   `-- 06f78a28f5e967f9b09e77d91b88fc524de347
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
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  36 directories, 22 files
