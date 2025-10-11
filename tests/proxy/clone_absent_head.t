  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

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

  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}/remote/real_repo.git/

  $ cat HEAD
  ref: refs/heads/master

  $ cat > HEAD <<EOF
  > ref: refs/does/not/exist
  > EOF


  $ cat HEAD
  ref: refs/does/not/exist

  $ cd ${TESTTMP}

  $ git clone http://localhost:8001/real_repo.git warning_clone
  Cloning into 'warning_clone'...

  $ git clone http://localhost:8002/real_repo.git full_repo
  Cloning into 'full_repo'...

  $ cd full_repo

  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file

  $ cat sub1/file1
  contents1

  $ cd ${TESTTMP}/remote/real_repo.git

  $ rm refs/heads/master

  $ cd ${TESTTMP}

  $ git clone http://localhost:8002/real_repo.git full_repo_no_master
  Cloning into 'full_repo_no_master'...
  warning: You appear to have cloned an empty repository.

  $ bash ${TESTDIR}/destroy_test_env.sh
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
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
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
  
  27 directories, 15 files

$ cat ${TESTTMP}/josh-proxy.out
