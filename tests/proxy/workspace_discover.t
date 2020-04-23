  $ source ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real/repo2.git
  warning: You appear to have cloned an empty repository.

  $ curl -s http://localhost:8002/version
  Version: * (glob)

  $ cd repo2

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir ws2
  $ cat > ws2/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add ws
  $ git add ws2
  $ git commit -m "add workspace" &> /dev/null

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

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" &> /dev/null

  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" &> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" &> /dev/null

  $ git push
  To http://localhost:8001/real/repo2.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real/repo2.git:workspace=ws.git ws

  $ sleep 10

  $ curl -s http://localhost:8002/views
  "real/repo2.git" = [
      ':/sub1',
      ':/sub1/subsub',
      ':/sub2',
      ':/sub3',
      ':/ws',
      ':/ws2',
      ':workspace=ws',
      ':workspace=ws2',
  ]

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   `-- filtered
  |       `-- real%2Frepo2.git
  |           |-- %3A%2Fsub1
  |           |   `-- heads
  |           |       `-- master
  |           |-- %3A%2Fsub1%2Fsubsub
  |           |   `-- heads
  |           |       `-- master
  |           |-- %3A%2Fsub2
  |           |   `-- heads
  |           |       `-- master
  |           |-- %3A%2Fsub3
  |           |   `-- heads
  |           |       `-- master
  |           |-- %3A%2Fws
  |           |   `-- heads
  |           |       `-- master
  |           |-- %3A%2Fws2
  |           |   `-- heads
  |           |       `-- master
  |           |-- %3Aworkspace=ws
  |           |   `-- heads
  |           |       `-- master
  |           `-- %3Aworkspace=ws2
  |               `-- heads
  |                   `-- master
  |-- namespaces
  |   `-- real%2Frepo2.git
  |       `-- refs
  |           `-- heads
  |               `-- master
  `-- tags
  
  25 directories, 9 files

$ cat ${TESTTMP}/josh-proxy.out
