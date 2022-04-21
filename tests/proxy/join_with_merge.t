  $ . ${TESTDIR}/setup_test_env.sh


  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ git checkout -b master
  Switched to a new branch 'master'
  $ echo content1 > file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ git checkout -b new1
  Switched to a new branch 'new1'
  $ echo content > newfile1 1> /dev/null
  $ git add .
  $ git commit -m "add newfile1" 1> /dev/null

  $ git checkout master 1> /dev/null
  Switched to branch 'master'
  $ echo content > newfile_master 1> /dev/null
  $ git add .
  $ git commit -m "newfile master" 1> /dev/null

  $ git merge -q new1 --no-ff

  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ git fetch --force http://localhost:8002/real_repo.git:prefix=sub1.git master:joined 1> /dev/null
  From http://localhost:8002/real_repo.git:prefix=sub1
   * [new branch]      master     -> joined

  $ git checkout joined
  Switched to branch 'joined'

  $ git log --graph --pretty=%s
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial

  $ tree
  .
  `-- sub1
      |-- file1
      |-- newfile1
      `-- newfile_master
  
  1 directory, 3 files


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':prefix=sub1']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       `-- %3Aprefix=sub1
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  11 directories, 3 files
