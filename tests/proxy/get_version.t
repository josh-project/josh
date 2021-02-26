  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ curl -s http://localhost:8002/version
  Version: 0.3.0


  $ curl -s "http://localhost:8002/~/graphql?query=\{version\}"
  {
    "data": {
      "version": "0.3.0"
    }
  } (no-eol)

  $ bash ${TESTDIR}/destroy_test_env.sh
  refs
  |-- heads
  `-- tags
  
  2 directories, 0 files
