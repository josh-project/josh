  $ cat > users.yaml <<EOF 
  > anonymous:
  >    groups: ['devel']
  > EOF
  $ cat > groups.yaml <<EOF 
  > /real_repo.git:
  >     devel:
  >         whitelist: |
  >             :[
  >                 ::sub1/
  >                 ::sub2/
  >                 ::whitelisted/
  >                 ::blacklisted/
  >                 ::whiteandblack/
  >             ]
  >         blacklist: "::sub1/file1"
  > EOF

  $ JOSH_USERS=$(pwd)/users.yaml JOSH_GROUPS=$(pwd)/groups.yaml . ${TESTDIR}/setup_test_env.sh

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

# only whitelisted files which are not blacklisted
  $ mkdir whitelisted
  $ cat > whitelisted/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1:exclude[::file1]
  > EOF

  $ git add whitelisted
  $ git commit -m "add whitelisted" 1> /dev/null

# whitelisted and blacklisted files
  $ mkdir whiteandblack
  $ cat > whiteandblack/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add whiteandblack
  $ git commit -m "add whiteandblack" 1> /dev/null

# only blacklisted files
  $ mkdir blacklisted
  $ cat > blacklisted/workspace.josh <<EOF
  > ::sub1/file1
  > EOF

  $ git add blacklisted
  $ git commit -m "add blacklisted" 1> /dev/null


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
  $ git commit -m "sub1 subsub file1" 1> /dev/null

  $ echo content2 > sub1/file1 1> /dev/null
  $ git add .
  $ git commit -m "sub1 file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null


  $ git log --graph --pretty=%s
  * add file2
  * sub1 file1
  * sub1 subsub file1
  * add file3
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial
  * add blacklisted
  * add whiteandblack
  * add whitelisted

  $ tree
  .
  |-- blacklisted
  |   `-- workspace.josh
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   |-- file1
  |   `-- subsub
  |       `-- file1
  |-- sub2
  |   `-- file2
  |-- sub3
  |   `-- file3
  |-- whiteandblack
  |   `-- workspace.josh
  `-- whitelisted
      `-- workspace.josh
  
  7 directories, 10 files


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

# whitelisted works
  $ git clone -q http://localhost:8002/real_repo.git:workspace=whitelisted.git whitelisted
  $ tree whitelisted
  whitelisted
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  4 directories, 3 files

# the others do not work
  $ git clone -q http://localhost:8002/real_repo.git:workspace=whiteandblack.git whiteandblack
  warning: You appear to have cloned an empty repository.
  $ git clone -q http://localhost:8002/real_repo.git:workspace=blacklisted.git blacklisted
  warning: You appear to have cloned an empty repository.

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/blacklisted',
      ':/sub1',
      ':/sub1/subsub',
      ':/sub2',
      ':/sub3',
      ':/whiteandblack',
      ':/whitelisted',
      ':workspace=blacklisted',
      ':workspace=whiteandblack',
      ':workspace=whitelisted',
  ]
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fblacklisted
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub1%2Fsubsub
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub2
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub3
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fwhiteandblack
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fwhitelisted
  |   |       |   `-- HEAD
  |   |       |-- %3Aworkspace=blacklisted
  |   |       |   `-- HEAD
  |   |       |-- %3Aworkspace=whiteandblack
  |   |       |   `-- HEAD
  |   |       `-- %3Aworkspace=whitelisted
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  20 directories, 12 files

