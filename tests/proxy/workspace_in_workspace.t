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

  $ git merge new1 --no-ff
  Merge made by the 'recursive' strategy.
   newfile1 | 0
   1 file changed, 0 insertions(+), 0 deletions(-)
   create mode 100644 newfile1

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

  $ git checkout HEAD~1 1> /dev/null
  Note: switching to 'HEAD~1'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at 833812f add file1

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
  POST git-receive-pack (402 bytes)
  remote: warning: ignoring broken ref refs/namespaces/request_1d2dd6d4-2013-46b4-b64f-d14f72ba153a/HEAD        
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
  Pushing to http://localhost:8002/real_repo.git:workspace=ws2.git
  POST git-receive-pack (459 bytes)
  error: RPC failed; curl 7 Failed to connect to localhost port 8002: Connection refused
  fatal: the remote end hung up unexpectedly
  fatal: the remote end hung up unexpectedly
  

  $ git reset --hard HEAD~1
  HEAD is now at b3be5ad add ws2
  $ echo ":workspace=sub1" >> workspace.josh
  $ git commit -a -m "sub1 as workspace in workspace"
  [master 1e246a6] sub1 as workspace in workspace
   1 file changed, 1 insertion(+)
  $ git sync
  Pushing to http://localhost:8002/real_repo.git:workspace=ws2.git
  fatal: unable to access 'http://localhost:8002/real_repo.git:workspace=ws2.git/': Failed to connect to localhost port 8002: Connection refused
  
  $ tree
  .
  |-- file1
  `-- workspace.josh
  
  0 directories, 2 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  josh-proxy: no process found
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3Aworkspace=ws
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3Aworkspace=ws2
  |   |           `-- heads
  |   |               `-- master
  |   |-- rewrites
  |   |   `-- real_repo.git
  |   |       |-- 191ead67feb541c237317e25b2c66c5d8f3e33fa
  |   |       |   `-- r_b3be5ad252e0f493a404a8785653065d7e677f21
  |   |       `-- 7bd92d97e96693ea7fd7eb5757b3580002889948
  |   |           `-- r_2cbcd105ead63a4fecf486b949db7f44710300e5
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  |   `-- request_7621f2a1-b72b-4e66-9d47-460ec408a67e
  |       |-- HEAD
  |       |-- push_options
  |       `-- refs
  |           |-- heads
  |           |   `-- master
  |           `-- josh
  |               `-- rewrites
  |                   `-- real_repo.git
  |                       `-- 191ead67feb541c237317e25b2c66c5d8f3e33fa
  |                           `-- r_b3be5ad252e0f493a404a8785653065d7e677f21
  `-- tags
  
  25 directories, 9 files

$ cat ${TESTTMP}/josh-proxy.out
