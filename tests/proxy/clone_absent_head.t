  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) bb282e9] add file1
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

  $ cd ${TESTTMP}/remote/real_repo.git/

  $ cat HEAD
  ref: refs/heads/master

  $ cat > HEAD <<EOF
  > ref: refs/does/not/exist
  > EOF


  $ cat HEAD
  ref: refs/does/not/exist

  $ cd ${TESTTMP}

  $ git clone http://localhost:8001/real_repo.git warning_clone
  Cloning into 'warning_clone'...
  warning: remote HEAD refers to nonexistent ref, unable to checkout.
  

  $ git clone http://localhost:8002/real_repo.git full_repo
  Cloning into 'full_repo'...

  $ cd full_repo

  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ cat sub1/file1
  contents1

  $ cd ${TESTTMP}/remote/real_repo.git

  $ rm refs/heads/master

  $ cd ${TESTTMP}

  $ git clone http://localhost:8002/real_repo.git full_repo_no_master
  Cloning into 'full_repo_no_master'...

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub1']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3A%2Fsub1
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

