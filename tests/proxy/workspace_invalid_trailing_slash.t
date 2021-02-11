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


  $ echo content1 > file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null


  $ mkdir -p sub1/subsub1
  $ echo contents1 > sub1/subsub1/file1
  $ git add .
  $ git commit -m "add subsub1" 1> /dev/null

  $ mkdir -p sub1/subsub2
  $ echo contents1 > sub1/subsub2/file1
  $ git add .
  $ git commit -m "add subsub2" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/ws
  $ cat > workspace.josh <<EOF
  > a/subsub1 = :/sub1/subsub1
  > a/subsub2 = :/sub1/subsub2
  > EOF

  $ git add .
  $ git commit -m "add workspace" 1> /dev/null
  $ git push origin HEAD:refs/heads/master -o merge 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: warning: ignoring broken ref refs/namespaces/* (glob)
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:    81c59c0..ae64c76  JOSH_PUSH -> master
  remote: REWRITE(85ee20960c56619305e098b301d8253888b6ce5b -> c255706f564f629eed1756b789d761048cfe060a)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new branch]      HEAD -> master

  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git pull -q --rebase

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add subsub2
  | * add subsub1
  * add workspace

  $ tree
  .
  |-- a
  |   |-- subsub1
  |   |   `-- file1
  |   `-- subsub2
  |       `-- file1
  `-- workspace.josh
  
  3 directories, 3 files
  $ cat workspace.josh
  a = :/sub1:[
      ::subsub1/
      ::subsub2/
  ]

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     81c59c0..ae64c76  master     -> origin/master
  Updating 81c59c0..ae64c76
  Fast-forward
   ws/workspace.josh | 4 ++++
   1 file changed, 4 insertions(+)
   create mode 100644 ws/workspace.josh

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add workspace
  * add subsub2
  * add subsub1
  * initial

  $ cd ${TESTTMP}/ws
  $ cat > workspace.josh <<EOF
  > a/ = :/sub1
  > EOF

  $ git add workspace.josh
  $ git commit -m "mod workspace" 1> /dev/null

  $ git log --graph --pretty=%s
  * mod workspace
  *   Merge from :workspace=ws
  |\  
  | * add subsub2
  | * add subsub1
  * add workspace


  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote:
  remote: Can't apply "mod workspace" (4e531443c5533e6d1b2503d0fad238cfc8491807)
  remote: Invalid workspace:
  remote: ----
  remote: a/ = :/sub1
  remote:
  remote: ----
  remote:
  remote:
  remote: error: hook declined to update refs/heads/master
  To http://localhost:8002/real_repo.git:workspace=ws.git
   ! [remote rejected] master -> master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:workspace=ws.git'


  $ cd ${TESTTMP}/ws
  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git pull --rebase
  Current branch master is up to date.

  $ tree
  .
  |-- a
  |   |-- subsub1
  |   |   `-- file1
  |   `-- subsub2
  |       `-- file1
  `-- workspace.josh
  
  3 directories, 3 files

  $ git log --graph --pretty=%s
  * mod workspace
  *   Merge from :workspace=ws
  |\  
  | * add subsub2
  | * add subsub1
  * add workspace

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub1/subsub1',
      ':/sub1/subsub2',
      ':/ws',
      ':workspace=ws',
  ]
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fsub1%2Fsubsub1
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fsub1%2Fsubsub2
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fws
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3Aworkspace=ws
  |   |           `-- heads
  |   |               `-- master
  |   |-- rewrites
  |   |   `-- real_repo.git
  |   |       `-- r_c255706f564f629eed1756b789d761048cfe060a
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  22 directories, 7 files
