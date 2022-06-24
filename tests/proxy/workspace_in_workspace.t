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

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add ws
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


  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * add file3
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial
  * add workspace


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  $ cd ws
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

  $ cat workspace.josh
  a/b = :/sub2
  c = :/sub1

  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * add workspace

  $ git checkout -q HEAD~1 1> /dev/null

  $ tree
  .
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  2 directories, 2 files

  $ git checkout master 1> /dev/null
  Previous HEAD position was 833812f add file1
  Switched to branch 'master'

  $ echo newfile_1_contents > c/subsub/newfile_1
  $ echo newfile_2_contents > a/b/newfile_2

  $ git add .

  $ git commit -m "add in filter" 1> /dev/null

  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:    176e8e0..11e2559  JOSH_PUSH -> master
  remote: REWRITE(5fa942ed9d35f280b35df2c4ef7acd23319271a5 -> 2cbcd105ead63a4fecf486b949db7f44710300e5)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:workspace=ws.git
     6be0d68..5fa942e  master -> master

  $ cd ${TESTTMP}/real_repo

  $ git pull --rebase 1> /dev/null
  From http://localhost:8001/real_repo
     176e8e0..11e2559  master     -> origin/master

  $ git clean -ffdx 1> /dev/null

  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   `-- subsub
  |       |-- file1
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  |-- sub3
  |   `-- file3
  `-- ws
      `-- workspace.josh
  
  5 directories, 9 files

  $ cat ws/workspace.josh
  c = :/sub1
  a/b = :/sub2

  $ git log --graph --pretty=%s
  * add in filter
  * add file2
  * add file1
  * add file3
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial
  * add workspace

  $ cd ..
  $ git clone http://localhost:8002/real_repo.git:workspace=ws2.git ws2
  Cloning into 'ws2'...
  warning: You appear to have cloned an empty repository.
  $ cd ws2
  $ echo "::file1" > workspace.josh
  $ git add workspace.josh
  $ git commit -m "add ws2"
  [master (root-commit) d31a8dc] add ws2
   1 file changed, 1 insertion(+)
   create mode 100644 workspace.josh
  $ git sync -o create
  * refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws2
   * branch            b3be5ad252e0f493a404a8785653065d7e677f21 -> FETCH_HEAD
  HEAD is now at b3be5ad add ws2
  Pushing to http://localhost:8002/real_repo.git:workspace=ws2.git
  POST git-receive-pack (424 bytes)
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    11e2559..517813c  JOSH_PUSH -> master        
  remote: REWRITE(d31a8dce16b9b197a1411e750602e62d8d2f97ae -> b3be5ad252e0f493a404a8785653065d7e677f21)        
  remote: 
  remote: 
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ echo ":workspace=ws" >> workspace.josh
  $ git commit -a -m "add workspace filter"
  [master e2532f1] add workspace filter
   1 file changed, 1 insertion(+)
  $ git sync
  ! refs/heads/master -> refs/heads/master
  Pushing to http://localhost:8002/real_repo.git:workspace=ws2.git
  POST git-receive-pack (481 bytes)
  remote: josh-proxy        
  remote: response from upstream:        
  remote: 
  remote: Can't apply "add workspace filter" (e2532f1207290ed9a961f9fc377a6b7afe415312)        
  remote: Invalid workspace: not reversible        
  remote: 
  remote: 
  remote: error: hook declined to update refs/heads/master        
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:workspace=ws2.git'
  

  $ git ls-tree -r HEAD
  100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\tfile1 (esc)
  100644 blob 2fa28dd621122cd858cf13f53f88dfe37eef5424\tworkspace.josh (esc)


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
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
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub1%2Fsubsub
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub2
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fsub3
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fws
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fws2
  |   |       |   `-- HEAD
  |   |       |-- %3Aworkspace=ws
  |   |       |   `-- HEAD
  |   |       `-- %3Aworkspace=ws2
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  18 directories, 10 files

