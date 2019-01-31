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

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git!+/pre.git pre
  $ cd pre

  $ echo contents2 > pre/file2
  $ git add .
  $ git commit -m "add file2" &> /dev/null
  $ git push &> /dev/null

  $ cd ${TESTTMP}/real_repo
  $ git pull
  From http://localhost:8001/real_repo
     *..*  master     -> origin/master (glob)
  Updating *..* (glob)
  Fast-forward
   file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 file2
  Current branch master is up to date.

  $ tree
  .
  |-- file2
  `-- sub1
      `-- file1
  
  1 directory, 2 files

  $ bash ${TESTDIR}/destroy_test_env.sh

