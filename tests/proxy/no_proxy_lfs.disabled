  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo
  $ git lfs install
  Updated Git hooks.
  Git LFS initialized.
  $ git lfs track "*.large"
  Tracking "*.large"

  $ git status
  On branch master
  
  No commits yet
  
  Untracked files:
    (use "git add <file>..." to include in what will be committed)
  \t.gitattributes (esc)
  
  nothing added to commit but untracked files present (use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1.large
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) 086980a] add file1
   1 file changed, 3 insertions(+)
   create mode 100644 sub1/file1.large

  $ tree
  .
  `-- sub1
      `-- file1.large
  
  2 directories, 1 file

  $ git config lfs.http://localhost:8001/real_repo.git/info/lfs.locksverify false

  $ git lfs push origin master > /dev/null
  $ git lfs logs last
  No logs to show

  $ git push > /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ bash ${TESTDIR}/destroy_test_env.sh
  .
  |-- josh
  |   `-- 22
  |       `-- sled
  |           |-- blobs
  |           |-- conf
  |           `-- db
  |-- mirror
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
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
          `-- tags
  
  21 directories, 10 files
