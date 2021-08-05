  $ . ${TESTDIR}/setup_test_env.sh


  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

# monorepo: client_master, our_master and dev point to the same commit,
# they have the following structure:
# sub1
#    file1
# sub2
#    file2
  $ git checkout -b client_master
  Switched to a new branch 'client_master'
  $ mkdir sub1
  $ echo content1 > sub1/file1 1> /dev/null
  $ mkdir sub2
  $ echo content2 > sub2/file2 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null
  $ git push origin client_master
  To http://localhost:8001/real_repo.git
   * [new branch]      client_master -> client_master
  $ git checkout -b our_master
  Switched to a new branch 'our_master'
  $ git push origin our_master
  To http://localhost:8001/real_repo.git
   * [new branch]      our_master -> our_master
  $ git checkout -b dev
  Switched to a new branch 'dev'
  $ git push origin dev
  To http://localhost:8001/real_repo.git
   * [new branch]      dev -> dev

# monorepo: commit in client_master
  $ git checkout client_master
  Switched to branch 'client_master'
  $ tree
  .
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- file2
  
  2 directories, 2 files
  $ echo content2 > sub1/file1
  $ git add .
  $ git commit -m "update file 1"
  [client_master e9e796f] update file 1
   1 file changed, 1 insertion(+)
  $ git status
  On branch client_master
  nothing to commit, working tree clean
  $ git log --graph --oneline
  * e9e796f update file 1
  * 9954c1c initial
  $ git push origin client_master 1> /dev/null
  To http://localhost:8001/real_repo.git
     9954c1c..e9e796f  client_master -> client_master

# monorepo: client_master is merged into our_master, unrelated history, no FF (so merge is visible)
  $ git checkout our_master
  Switched to branch 'our_master'
  $ git merge client_master --allow-unrelated-histories --no-ff -X theirs
  Merge made by the 'recursive' strategy.
   sub1/file1 | 1 +
   1 file changed, 1 insertion(+)
  $ git status
  On branch our_master
  nothing to commit, working tree clean
  $ git log --graph --oneline
  *   1502194 Merge branch 'client_master' into our_master
  |\  
  | * e9e796f update file 1
  |/  
  * 9954c1c initial
  $ git push origin our_master 1> /dev/null
  To http://localhost:8001/real_repo.git
     9954c1c..1502194  our_master -> our_master

# monorepo: our_master is merged in dev, with no fast forward
  $ git checkout dev
  Switched to branch 'dev'
  $ git merge our_master --allow-unrelated-histories -X theirs --no-ff
  Merge made by the 'recursive' strategy.
   sub1/file1 | 1 +
   1 file changed, 1 insertion(+)
  $ git status
  On branch dev
  nothing to commit, working tree clean
  $ git log --graph --oneline
  *   e47d5ed Merge branch 'our_master' into dev
  |\  
  | * 1502194 Merge branch 'client_master' into our_master
  |/| 
  | * e9e796f update file 1
  |/  
  * 9954c1c initial
  $ git status sub2
  On branch dev
  nothing to commit, working tree clean
  $ git log --graph --oneline sub2 
  * 9954c1c initial
  $ git push origin dev 1> /dev/null
  To http://localhost:8001/real_repo.git
     9954c1c..e47d5ed  dev -> dev

# log of sub1 folder looks clean
  $ git log --graph --oneline sub1
  * e9e796f update file 1
  * 9954c1c initial

# extracted sub1 repo: dev looks dirty (unwanted merge)
  $ cd ..
  $ git clone http://localhost:8002/real_repo.git:/sub1.git -b dev
  Cloning into 'sub1'...
  $ cd sub1
  $ git status
  On branch dev
  Your branch is up to date with 'origin/dev'.
  
  nothing to commit, working tree clean
  $ git log --graph --oneline
  *   9feef90 Merge branch 'our_master' into dev
  |\  
  | * 3e2d8d2 Merge branch 'client_master' into our_master
  |/| 
  | * 3726548 update file 1
  |/  
  * eb6a311 initial

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub2',
  ]
  refs
  |-- heads
  |-- josh
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   |-- client_master
  |                   |-- dev
  |                   `-- our_master
  |-- namespaces
  `-- tags
  
  8 directories, 3 files
