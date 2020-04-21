  $ source ${TESTDIR}/setup_test_env.sh

  $ cd ${TESTTMP}
  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real_repo.git &> /dev/null
  $ cd real_repo
  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null
  $ git push &> /dev/null

  $ cd ${TESTTMP}
  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real/repo2.git real_repo2 &> /dev/null
  $ cd real_repo2
  $ mkdir sub1
  $ echo contents1_repo2 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null
  $ git push &> /dev/null

  $ cd ${TESTTMP}
  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git:/sub1.git
  $ cd sub1
  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "add file2" &> /dev/null
  $ git push &> /dev/null

This uses a repo that has a path with more than one element, causing nested namespaces.
  $ cd ${TESTTMP}
  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real/repo2.git:/sub1.git sub1_repo2
  $ cd sub1_repo2

Put a double slash in the URL to see that it also works
  $ git fetch http://${TESTUSER}:${TESTPASS}@localhost:8002/real//repo2.git:/sub1.git
  From http://localhost:8002/real//repo2.git:/sub1
   * branch            HEAD       -> FETCH_HEAD

  $ git diff HEAD FETCH_HEAD

  $ echo contents2_repo2 > file2
  $ git add file2
  $ git commit -m "add file2" &> /dev/null
  $ git push &> /dev/null

  $ cd ${TESTTMP}/real_repo
  $ git pull
  From http://localhost:8001/real_repo
     *..*  master     -> origin/master (glob)
  Updating *..* (glob)
  Fast-forward
   sub1/file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2
  Current branch master is up to date.

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  1 directory, 2 files

  $ cat sub1/file2
  contents2

  $ cd ${TESTTMP}/real_repo2
  $ git pull
  From http://localhost:8001/real/repo2
     *..*  master     -> origin/master (glob)
  Updating *..* (glob)
  Fast-forward
   sub1/file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file2
  Current branch master is up to date.

  $ tree
  .
  `-- sub1
      |-- file1
      `-- file2
  
  1 directory, 2 files

  $ cat sub1/file2
  contents2_repo2

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   `-- filtered
  |       |-- real%repo2.git
  |       |   `-- #%sub1
  |       |       `-- heads
  |       |           `-- master
  |       `-- real_repo.git
  |           `-- #%sub1
  |               `-- heads
  |                   `-- master
  |-- namespaces
  |   |-- real%repo2.git
  |   |   `-- refs
  |   |       `-- heads
  |   |           `-- master
  |   `-- real_repo.git
  |       `-- refs
  |           `-- heads
  |               `-- master
  `-- tags
  
  17 directories, 4 files

