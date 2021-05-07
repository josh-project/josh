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


  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add ws
  $ git commit -m "add workspace" 1> /dev/null

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
  * add workspace
  * add file2
  * add file1
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws:prefix=pre:/pre.git ws
  $ cd ws
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
  * add workspace
  * add file2
  * add file1

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
  
  HEAD is now at a4b6822 add workspace
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
  Previous HEAD position was a4b6822 add workspace
  HEAD is now at 2a03ad0 add file2
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  `-- c
      `-- subsub
          `-- file1
  
  4 directories, 2 files

  $ git checkout master 1> /dev/null
  Previous HEAD position was 2a03ad0 add file2
  Switched to branch 'master'

  $ echo newfile_1_contents > c/subsub/newfile_1
  $ git rm c/subsub/file1
  rm 'c/subsub/file1'
  $ echo newfile_2_contents > a/b/newfile_2
  $ echo ws_file_contents > ws_file

  $ git add .

  $ git commit -m "add in filter" 1> /dev/null

  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:    aaec05d..edefd7d  JOSH_PUSH -> master
  remote: REWRITE(7de033196d3f74f40139647122f499286a97498b -> 44edc62d506b9805a3edfc74db15b1cc0bfc6871)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:workspace=ws:prefix=pre:/pre.git
     6712cb1..7de0331  master -> master

  $ git pull origin master --rebase 1>/dev/null
  From http://localhost:8002/real_repo.git:workspace=ws:prefix=pre:/pre
   * branch            master     -> FETCH_HEAD
   + 7de0331...44edc62 master     -> origin/master  (forced update)
  \r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  Successfully rebased and updated refs/heads/master.

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > w = :/sub3
  > EOF

  $ git add .
  $ git commit -m "try to modify ws" 1> /dev/null

  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:    edefd7d..18aaa0c  JOSH_PUSH -> master
  remote: REWRITE(5eedfbadb706c633671935d425d19b76d2374d34 -> 9d72b88b11aed97d3313f0a6d80894ee2ffdf3e9)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:workspace=ws:prefix=pre:/pre.git
     44edc62..5eedfba  master -> master
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase 2> /dev/null

Note that d/ is still in the tree but now it is not overlayed
  $ tree
  .
  |-- a
  |   `-- b
  |       |-- file2
  |       `-- newfile_2
  |-- c
  |   `-- subsub
  |       `-- newfile_1
  |-- d
  |   `-- file3
  |-- w
  |   `-- file3
  |-- workspace.josh
  `-- ws_file
  
  6 directories, 7 files



  $ cd ${TESTTMP}/real_repo

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase 1> /dev/null
  From http://localhost:8001/real_repo
     aaec05d..18aaa0c  master     -> origin/master

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
      |-- d
      |   `-- file3
      |-- workspace.josh
      `-- ws_file
  
  6 directories, 10 files
  $ git log --graph --pretty=%s
  * try to modify ws
  * add in filter
  * mod workspace
  * add file3
  * add workspace
  * add file2
  * add file1
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial

  $ cat sub1/subsub/file1
  cat: sub1/subsub/file1: No such file or directory
  [1]

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
  
  HEAD is now at edefd7d add in filter
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
  Previous HEAD position was edefd7d add in filter
  HEAD is now at aaec05d mod workspace
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
      ':/ws/d',
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
  |   |       |-- %3A%2Fsub1%2Fsubsub
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fsub2
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fsub3
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fws
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fws%2Fd
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3Aworkspace=ws
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3Aworkspace=ws%3Aprefix=pre%3A%2Fpre
  |   |           `-- heads
  |   |               `-- master
  |   |-- rewrites
  |   |   `-- real_repo.git
  |   |       `-- 5c1a6d0fe36ae7aa1727c8f071ae92d3645a63ea
  |   |           |-- r_44edc62d506b9805a3edfc74db15b1cc0bfc6871
  |   |           `-- r_9d72b88b11aed97d3313f0a6d80894ee2ffdf3e9
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  29 directories, 11 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
