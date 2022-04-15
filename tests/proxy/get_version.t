  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ curl -s http://localhost:8002/version
  Version: 22.4.15


  $ curl -s "http://localhost:8002/~/graphql?query=\{version\}"
  {
    "data": {
      "version": "22.4.15"
    }
  } (no-eol)

  $ bash ${TESTDIR}/destroy_test_env.sh
  refs
  |-- heads
  `-- tags
  
  2 directories, 0 files
