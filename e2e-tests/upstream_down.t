  $ source ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ killall josh-test-server &> /dev/null
  * (glob)

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real_repo.git
  fatal: unable to access 'http://*:*@localhost:8001/real_repo.git/': Failed to connect to localhost port 8001: Connection refused (glob)
  [128]

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git full_repo
  fatal: Authentication failed for 'http://*:*@localhost:8002/real_repo.git/' (glob)
  [128]

  $ cd full_repo
  /bin/sh: line 12: cd: full_repo: No such file or directory
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh &> /dev/null
