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
   * branch            4a199f3a19a292e6639dede0f8602afc19a82dfc -> FETCH_HEAD
  HEAD is now at 4a199f3 Merge from :workspace=ws
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (439 bytes)
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    5d605ce..98c996c  JOSH_PUSH -> master        
  remote: REWRITE(1b46698f32d1d1db1eaeb34f8c9037778d65f3a9 -> 4a199f3a19a292e6639dede0f8602afc19a82dfc)        
  remote: 
  remote: 
  updating local tracking ref 'refs/remotes/origin/master'
  

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + 1b46698...4a199f3 master     -> origin/master  (forced update)
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
  
  4 directories, 3 files

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add file2
  | * add file1
  * add workspace

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     5d605ce..98c996c  master     -> origin/master
  Updating 5d605ce..98c996c
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

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
  > EOF

  $ git add ws
  $ git commit -m "mod workspace" 1> /dev/null

  $ git log --graph --pretty=%s
  * mod workspace
  * add file3
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


  $ git sync
    refs/heads/master -> refs/heads/master
  Pushing to http://localhost:8001/real_repo.git
  POST git-receive-pack (790 bytes)
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cd ${TESTTMP}/ws
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
     4a199f3..4972ae1  master     -> origin/master
  Updating 4a199f3..4972ae1
  Fast-forward
   d/file3        | 1 +
   workspace.josh | 3 ++-
   2 files changed, 3 insertions(+), 1 deletion(-)
   create mode 100644 d/file3

  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  |-- d
  |   `-- file3
  `-- workspace.josh
  
  5 directories, 4 files

  $ git log --graph --pretty=%s
  *   mod workspace
  |\  
  | * add file3
  *   Merge from :workspace=ws
  |\  
  | * add file2
  | * add file1
  * add workspace

  $ git checkout -q HEAD~1 1> /dev/null
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  4 directories, 3 files

  $ git checkout HEAD~1 1> /dev/null
  Previous HEAD position was 4a199f3 Merge from :workspace=ws
  HEAD is now at 9441c1b add workspace
  $ tree
  .
  `-- workspace.josh
  
  0 directories, 1 file

  $ git checkout master 1> /dev/null
  Previous HEAD position was 9441c1b add workspace
  Switched to branch 'master'

  $ echo newfile_1_contents > c/subsub/newfile_1
  $ git rm c/subsub/file1
  rm 'c/subsub/file1'
  $ echo newfile_2_contents > a/b/newfile_2
  $ echo ws_file_contents > ws_file

  $ git add .

  $ git commit -m "add in filter" 1> /dev/null

  $ git sync
    refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            3136fff7280627623bf4d71191d1aea783579be0 -> FETCH_HEAD
  HEAD is now at 3136fff add in filter
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (808 bytes)
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    e41565c..9c41f84  JOSH_PUSH -> master        
  remote: REWRITE(6f33ff469334600e2a53433208efc1cd734b49b3 -> 3136fff7280627623bf4d71191d1aea783579be0)        
  remote: 
  remote: 
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > w = :/sub3
  > EOF

  $ git mv d w
  $ git add .
  $ git commit -m "try to modify ws" 1> /dev/null

  $ git sync
    refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            91e1e8645d3439b195f3866664092ebc20e63bb5 -> FETCH_HEAD
  HEAD is now at 91e1e86 try to modify ws
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (464 bytes)
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    9c41f84..21cd34c  JOSH_PUSH -> master        
  remote: REWRITE(9f8d9c0adcbc65e17dfb359c6e3dee7520649c88 -> 91e1e8645d3439b195f3866664092ebc20e63bb5)        
  remote: 
  remote: 
  updating local tracking ref 'refs/remotes/origin/master'
  

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + 9f8d9c0...91e1e86 master     -> origin/master  (forced update)
  Already up to date.

  $ tree
  .
  |-- a
  |   `-- b
  |       |-- file2
  |       `-- newfile_2
  |-- c
  |   `-- subsub
  |       `-- newfile_1
  |-- w
  |   `-- file3
  |-- workspace.josh
  `-- ws_file
  
  5 directories, 6 files

  $ cat workspace.josh
  c = :/sub1
  a/b = :/sub2
  w = :/sub3

  $ git log --graph --pretty=%s
  * try to modify ws
  * add in filter
  *   mod workspace
  |\  
  | * add file3
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
     e41565c..21cd34c  master     -> origin/master
  Updating e41565c..21cd34c
  Fast-forward
   sub1/subsub/file1     | 1 -
   sub1/subsub/newfile_1 | 1 +
   sub2/newfile_2        | 1 +
   ws/workspace.josh     | 4 ++--
   ws/ws_file            | 1 +
   5 files changed, 5 insertions(+), 3 deletions(-)
   delete mode 100644 sub1/subsub/file1
   create mode 100644 sub1/subsub/newfile_1
   create mode 100644 sub2/newfile_2
   create mode 100644 ws/ws_file

  $ git clean -ffdx 1> /dev/null

Note that ws/d/ is now present in the ws
  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   `-- subsub
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  |-- sub3
  |   `-- file3
  `-- ws
      |-- workspace.josh
      `-- ws_file
  
  5 directories, 9 files
  $ git log --graph --pretty=%s
  * try to modify ws
  * add in filter
  * mod workspace
  * add file3
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


  $ git checkout -q HEAD~1 1> /dev/null
  $ git clean -ffdx 1> /dev/null
  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   `-- subsub
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  |-- sub3
  |   `-- file3
  `-- ws
      |-- workspace.josh
      `-- ws_file
  
  5 directories, 9 files

  $ git checkout HEAD~1 1> /dev/null
  Previous HEAD position was 9c41f84 add in filter
  HEAD is now at e41565c mod workspace
  $ git clean -ffdx 1> /dev/null
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
  |-- sub3
  |   `-- file3
  `-- ws
      `-- workspace.josh
  
  5 directories, 7 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub1/subsub',
      ':/sub2',
      ':/sub3',
      ':/ws',
      ':workspace=ws',
  ]
  refs
  |-- heads
  |-- josh
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  8 directories, 2 files

$ cat ${TESTTMP}/josh-proxy.out
