  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:prefix=pre.git pre

  $ cd ${TESTTMP}/real_repo
  $ echo x > sub1/filex
  $ git add .
  $ git commit -q -m "filex"
  $ git push -q

  $ cd ${TESTTMP}/pre

  $ echo contents2 > pre/file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null
  $ git push -q
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:  ! [rejected]        JOSH_PUSH -> master (fetch first)        
  remote: error: failed to push some refs to 'http://*localhost:8001/real_repo.git'* (glob)
  remote: hint: Updates were rejected because the remote contains work that you do        
  remote: hint: not have locally. This is usually caused by another repository pushing        
  remote: hint: to the same ref. You may want to first integrate the remote changes        
  remote: hint: (e.g., 'git pull ...') before pushing again.        
  remote: hint: See the 'Note about fast-forwards' in 'git push --help' for details.        
  remote: 
  remote: 
  remote: error: hook declined to update refs/heads/master        
  To http://localhost:8002/real_repo.git:prefix=pre.git
   ! [remote rejected] master -> master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:prefix=pre.git'
  [1]

