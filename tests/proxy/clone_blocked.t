  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/blocked_repo.git
  remote: Access to this repo is blocked via JOSH_REPO_BLOCK
  fatal: unable to access 'http://localhost:8002/blocked_repo.git/': The requested URL returned error: 422
  [128]

  $ bash ${TESTDIR}/destroy_test_env.sh
  refs
  |-- heads
  `-- tags
  
  2 directories, 0 files

