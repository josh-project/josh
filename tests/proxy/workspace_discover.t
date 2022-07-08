  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real/repo2.git
  warning: You appear to have cloned an empty repository.


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
  $ git commit -m "add workspace" 1> /dev/null

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

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ git push
  To http://localhost:8001/real/repo2.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real/repo2.git:workspace=ws.git ws

  $ sleep 10

  $ curl -s http://localhost:8002/filters
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
  refs
  |-- heads
  |-- josh
  |   `-- upstream
  |       `-- real%2Frepo2.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  8 directories, 2 files

$ cat ${TESTTMP}/josh-proxy.out
