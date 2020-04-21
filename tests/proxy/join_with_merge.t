  $ source ${TESTDIR}/setup_test_env.sh


  $ cd ${TESTTMP}
  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real_repo.git &> /dev/null
  $ cd real_repo

  $ git checkout -b master
  Switched to a new branch 'master'
  $ echo content1 > file1 &> /dev/null
  $ git add .
  $ git commit -m "initial" &> /dev/null

  $ git checkout -b new1
  Switched to a new branch 'new1'
  $ echo content > newfile1 &> /dev/null
  $ git add .
  $ git commit -m "add newfile1" &> /dev/null

  $ git checkout master &> /dev/null
  $ echo content > newfile_master &> /dev/null
  $ git add .
  $ git commit -m "newfile master" &> /dev/null

  $ git merge new1 --no-ff
  Merge made by the 'recursive' strategy.
   newfile1 | 0
   1 file changed, 0 insertions(+), 0 deletions(-)
   create mode 100644 newfile1

  $ git push &> /dev/null

  $ git fetch --force http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git:prefix=sub1.git master:joined &> /dev/null

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
  remote/scratch/refs
  |-- heads
  |-- josh
  |   `-- filtered
  |       `-- real_repo.git
  |           `-- #prefix=sub1
  |               `-- heads
  |                   `-- master
  |-- namespaces
  |   `-- real_repo.git
  |       `-- refs
  |           `-- heads
  |               `-- master
  `-- tags
  
  11 directories, 2 files
