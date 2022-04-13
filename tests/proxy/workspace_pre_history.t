Worspaces should also contain the history of the main directory before the workspace.josh
file was created


  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ curl -s http://localhost:8002/version
  Version: 0.3.0

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir ws

  $ echo content1 > ws/file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add ws
  $ git commit -m "add workspace" 1> /dev/null

  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file1
  * add workspace
  * initial


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  $ cd ws
  $ tree
  .
  |-- c
  |   `-- subsub
  |       `-- file1
  |-- file1
  `-- workspace.josh
  
  2 directories, 3 files

  $ git log --graph --pretty=%s
  * add file1
  * add workspace
  * initial

  $ git checkout -q HEAD~1 1> /dev/null

  $ tree
  .
  |-- file1
  `-- workspace.josh
  
  0 directories, 2 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub1/subsub',
      ':/ws',
      ':workspace=ws',
  ]
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub1%2Fsubsub
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fws
  |   |       |   `-- HEAD
  |   |       `-- %3Aworkspace=ws
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  14 directories, 6 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
