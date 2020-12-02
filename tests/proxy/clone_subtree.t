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
  [master (root-commit) *] add file1 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2"
  [master *] add file2 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file2

  $ tree
  .
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- file2
  
  2 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * add file1

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git sub1

  $ cd sub1
  $ cat .git/refs/remotes/origin/HEAD
  ref: refs/remotes/origin/master

  $ tree
  .
  `-- file1
  
  0 directories, 1 file

  $ git log --graph --pretty=%s
  * add file1

  $ cat file1
  contents1

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3A%2Fsub2
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  14 directories, 3 files
