  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/real_repo

  $ echo foo > bla
  $ git add .
  $ git commit -m "initial"
  [master (root-commit) *] initial (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 bla

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git

  $ cd ${TESTTMP}/real_repo
  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add .
  $ git commit -m "unrelated on master" 1> /dev/null
  $ git push origin HEAD:refs/heads/master 1> /dev/null
  To http://localhost:8001/real_repo.git
     *..*  HEAD -> master (glob)

  $ cd ${TESTTMP}/sub1
  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git fetch

  $ echo contents2 > file4
  $ git add .
  $ git commit -m "add file4" 1> /dev/null
  $ git push origin master:refs/heads/from_filtered 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote:  To http://localhost:8001/real_repo.git
  remote:  * [new branch]      JOSH_PUSH -> from_filtered
  remote:
  remote:
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      master -> from_filtered

  $ git push origin master:refs/heads/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote:  To http://localhost:8001/real_repo.git
  remote:    *..*  JOSH_PUSH -> master (glob)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:/sub1.git
     *..*  master -> master (glob)

  $ cd ${TESTTMP}/real_repo
  $ git fetch
  From http://localhost:8001/real_repo
     *..*  master        -> origin/master (glob)
   * [new branch]      from_filtered -> origin/from_filtered

  $ git log --graph --pretty=%s origin/master
  * add file4
  * unrelated on master
  * add file1
  * initial
  $ git log --graph --pretty=%s origin/from_filtered
  * add file4

  $ . ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3A%2Fsub2
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  14 directories, 3 files
