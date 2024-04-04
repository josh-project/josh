  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.


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


  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ git sync
  * refs/heads/master -> refs/heads/master
  Pushing to http://localhost:8001/real_repo.git
  POST git-receive-pack (1457 bytes)
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cd ${TESTTMP}
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/ws
  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add .
  $ git commit -m "add workspace" 1> /dev/null
  $ git sync origin HEAD:refs/heads/master -o merge
  * HEAD -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            d91fa4981fe3546f44fa5a779ec6f69b20fdaa0f -> FETCH_HEAD
  HEAD is now at d91fa49 Merge from :workspace=ws
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (439 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    5d605ce..0ebcca7  JOSH_PUSH -> master        
  remote: REWRITE(1b46698f32d1d1db1eaeb34f8c9037778d65f3a9 -> d91fa4981fe3546f44fa5a779ec6f69b20fdaa0f)        
  updating local tracking ref 'refs/remotes/origin/master'
  

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + 1b46698...d91fa49 master     -> origin/master  (forced update)
  Already up to date.

  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  5 directories, 3 files

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add file2
  | * add file1
  * add workspace

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     5d605ce..0ebcca7  master     -> origin/master
  Updating 5d605ce..0ebcca7
  Fast-forward
   ws/workspace.josh | 2 ++
   1 file changed, 2 insertions(+)
   create mode 100644 ws/workspace.josh

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add workspace
  * add file2
  * add file1
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial

  $ cd ${TESTTMP}/ws

  $ cat > workspace.josh <<EOF
  > a/c = :/sub2
  > c = :/sub1
  > EOF

  $ git mv a/b a/c
  $ git add workspace.josh
  $ git commit -m "mod workspace" 1> /dev/null

  $ git log --graph --pretty=%s
  * mod workspace
  *   Merge from :workspace=ws
  |\  
  | * add file2
  | * add file1
  * add workspace


  $ git sync
    refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            251b9a34c94c69defbebdfc716a7eff98df80b51 -> FETCH_HEAD
  HEAD is now at 251b9a3 mod workspace
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (530 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    0ebcca7..57d15ef  JOSH_PUSH -> master        
  remote: REWRITE(5d8563b5319ef897a6aa971c0e4dd275a1139bf5 -> 251b9a34c94c69defbebdfc716a7eff98df80b51)        
  updating local tracking ref 'refs/remotes/origin/master'
  

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + 5d8563b...251b9a3 master     -> origin/master  (forced update)
  Already up to date.

  $ tree
  .
  |-- a
  |   `-- c
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  5 directories, 3 files

  $ git log --graph --pretty=%s
  * mod workspace
  *   Merge from :workspace=ws
  |\  
  | * add file2
  | * add file1
  * add workspace

  $ cd ${TESTTMP}/real_repo

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8001/real_repo
     0ebcca7..57d15ef  master     -> origin/master
  Updating 0ebcca7..57d15ef
  Fast-forward
   ws/workspace.josh | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)

  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   `-- subsub
  |       `-- file1
  |-- sub2
  |   `-- file2
  `-- ws
      `-- workspace.josh
  
  5 directories, 6 files
  $ git log --graph --pretty=%s
  * mod workspace
  *   Merge from :workspace=ws
  |\  
  | * add workspace
  * add file2
  * add file1
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial
