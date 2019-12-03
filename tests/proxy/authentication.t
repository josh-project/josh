
  $ source ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://admin@localhost:8001/real_repo.git
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

  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ cat ${TESTTMP}/josh-test-server.out | grep CREDENTIALS | uniq
  CREDENTIALS OK "admin" ""

  $ git clone -q http://${TESTUSER}:wrongpass@localhost:8002/real_repo.git full_repo
  fatal: Authentication failed for 'http://localhost:8002/real_repo.git/'
  [128]


  $ rm -Rf full_repo

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git full_repo

  $ cat ${TESTTMP}/josh-test-server.out | grep CREDENTIALS | grep ${TESTUSER} | grep ${TESTPASS} | uniq
  CREDENTIALS OK "*" "*" (glob)

  $ cd full_repo
  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ cat sub1/file1
  contents1

  $ rm -Rf full_repo
  $ git clone -q http://x\':bla@localhost:8002/real_repo.git full_repo
  fatal: Authentication failed for 'http://localhost:8002/real_repo.git/'
  [128]
  $ cd full_repo
  /bin/sh: line 46: cd: full_repo: No such file or directory
  [1]
  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ bash ${TESTDIR}/destroy_test_env.sh &> /dev/null
