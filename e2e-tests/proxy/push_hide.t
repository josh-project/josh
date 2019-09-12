  $ source ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real_repo.git &> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" &> /dev/null
  $ git push &> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" &> /dev/null
  $ git push &> /dev/null

  $ cd ${TESTTMP}

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git:hide=sub2.git sub1
  $ cd sub1

  $ echo contents3 > sub1/file3
  $ git add sub1/file3
  $ git commit -m "add sub1/file3" &> /dev/null
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:    *..*  JOSH_PUSH -> master * (glob)
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:hide=sub2.git
     *..*  master -> master (glob)

  $ cd ${TESTTMP}/real_repo
  $ git pull
  From http://localhost:8001/real_repo
     *..*  master     -> origin/master (glob)
  Updating *..* (glob)
  Fast-forward
   sub1/file3 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file3
  Current branch master is up to date.

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file3
  `-- sub2
      `-- file2
  
  2 directories, 3 files

  $ cat sub1/file3
  contents3

  $ git log --graph --pretty=%s
  * add sub1/file3
  * add file2
  * add file1

Make sure all temporary namespace got removed
  $ tree ${TESTTMP}/remote/scratch/real_repo.git/refs/ | grep request_
  [1]

  $ bash ${TESTDIR}/destroy_test_env.sh

$ cat ${TESTTMP}/josh-proxy.out | grep REPO_UPDATE
