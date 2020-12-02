This is a bit of a special case:
Normally when selecting commits to be included in the rewritten history Josh excludes
all commits whose tree has no diff to the parent.
This makes sure that when extracting subdirectories for example, only commits that affect
the directory of interest are included in the history.
However in case the diff was also empty in the untransformed commit this means the commit
was creaded on purpose (by passing --allow-empty to git commit) and in this case the commit
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
  [master (root-commit) *] add file1 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ git commit --allow-empty -m "x"
  [master *] x (glob)

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2"
  [master *] add file2 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  1 directory, 2 files

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
  
  1 directory, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * x
  * add file1

  $ cat sub1/file1
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
  |   |       `-- %3Anop
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
