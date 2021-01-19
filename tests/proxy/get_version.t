  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

No Idea why this is needed...
  $ sleep 1

  $ curl -s http://localhost:8002/version
  Version: 0.3.0


  $ bash ${TESTDIR}/destroy_test_env.sh
  refs
  |-- heads
  `-- tags
  
  2 directories, 0 files
