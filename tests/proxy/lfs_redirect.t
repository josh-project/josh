  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ curl -is http://localhost:8002/real_repo.git/info/lfs/objects/batch | grep -v "date:"
  HTTP/1.1 307 Temporary Redirect\r (esc)
  location: http://localhost:8001/real_repo.git/info/lfs/objects/batch\r (esc)
  content-length: 0\r (esc)
  \r (esc)

  $ curl -is http://localhost:8002/real_repo.git/info/lfs | grep -v "date:"
  HTTP/1.1 307 Temporary Redirect\r (esc)
  location: http://localhost:8001/real_repo.git/info/lfs\r (esc)
  content-length: 0\r (esc)
  \r (esc)

  $ curl -is http://localhost:8002/real_repo.git@refs/heads/master:/sub1.git/info/lfs | grep -v "date:"
  HTTP/1.1 307 Temporary Redirect\r (esc)
  location: http://localhost:8001/real_repo.git/info/lfs\r (esc)
  content-length: 0\r (esc)
  \r (esc)