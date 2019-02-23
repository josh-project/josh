  $ source ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > EOF

  $ git add ws
  $ git commit -m "add workspace" &> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" &> /dev/null

  $ git log --graph --pretty=%s
  * add file2
  * add workspace


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://${TESTUSER}:${TESTPASS}@localhost:8002/real_repo.git:workspace=ws.git ws
  $ cd ws
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  `-- workspace.josh
  
  2 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * add workspace

  $ git checkout master &> /dev/null

  $ mkdir -p c/subsub
  $ echo newfile_1_contents > c/subsub/newfile_1
  $ echo newfile_2_contents > a/b/newfile_2

  $ git add .

  $ git commit -m "add in view" &> /dev/null

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add .
  $ git commit -m "publish" &> /dev/null

  $ git push &> /dev/null

  $ cd ${TESTTMP}/real_repo

  $ git pull &> /dev/null

  $ git clean -ffdx &> /dev/null

  $ tree
  .
  |-- sub1
  |   `-- subsub
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  `-- ws
      `-- workspace.josh
  
  4 directories, 4 files
  $ git log --graph --pretty=%s
  * publish
  * add in view
  * add file2
  * add workspace

  $ git checkout HEAD~1 &> /dev/null
  $ tree
  .
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  `-- ws
      |-- c
      |   `-- subsub
      |       `-- newfile_1
      `-- workspace.josh
  
  4 directories, 4 files

  $ bash ${TESTDIR}/destroy_test_env.sh

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
