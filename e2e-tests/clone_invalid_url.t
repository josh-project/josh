  $ source ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://testuser:supersafe@localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null
  $ git push &> /dev/null

  $ cd ${TESTTMP}

  $ git clone -q http://testuser:supersafe@localhost:8002/xxx full_repo
  fatal: repository 'http://testuser:supersafe@localhost:8002/xxx/' not found
  [128]


  $ bash ${TESTDIR}/destroy_test_env.sh &> /dev/null

