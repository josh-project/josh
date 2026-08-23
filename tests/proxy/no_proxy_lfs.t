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

  $ git-tree-pretty .
  .
  ├── .gitattributes
  │   ┆  *.large filter=lfs diff=lfs merge=lfs -text
  └── sub1/
      └── file1.large
          ┆  version https://git-lfs.github.com/spec/v1
          ┆  oid sha256:8f88da056e2ed130ee23b3b61245d2e0948fe335236dcb23a100a087f92130f2
          ┆  size 10

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
  |   `-- cache
  |       `-- 33
  |           `-- sled
  |               |-- blobs
  |               |-- conf
  |               `-- db
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
  
  22 directories, 10 files
