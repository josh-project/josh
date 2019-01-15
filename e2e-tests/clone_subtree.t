  $ bash ${TESTDIR}/setup_test_env.sh
  $ cd ${CRAMTMP}

  $ git clone -q http://testuser:supersafe@localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master

  No commits yet

  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) *] add file1 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ tree
  .
  `-- sub1
      `-- file1

  1 directory, 1 file

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${CRAMTMP}

  $ git clone -q http://testuser:supersafe@localhost:8002/real_repo.git/sub1.git

  $ cd sub1

  $ tree
  .
  `-- file1

  0 directories, 1 file
  $ bash ${TESTDIR}/destroy_test_env.sh

