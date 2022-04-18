  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ curl -s http://localhost:8002/version
  Version: 22.4.15

  $ bash ${TESTDIR}/destroy_test_env.sh
  refs
  |-- heads
  `-- tags
  
  2 directories, 0 files
