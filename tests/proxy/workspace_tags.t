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
  $ git pull 2>/dev/null 1>/dev/null
  $ git log --graph --oneline
  * 11e2559 add in filter
  * 176e8e0 add file2
  * 76cd9e6 add file1
  * 828956f add file3
  *   65ca339 Merge branch 'new1'
  |\  
  | * 902bb8f add newfile1
  * | f5719cb newfile master
  |/  
  * a75eedb initial
  * 8360d96 add workspace

# Pushing a tag from the workspace on the latest commit
  $ git tag tag_from_ws_1

  $ git push origin tag_from_ws_1 -o base=refs/heads/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  fatal: the receiving end does not support push options
  fatal: the remote end hung up unexpectedly
  error: failed to push some refs to 'http://localhost:8001/real_repo.git'

  $ cd ${TESTTMP}/real_repo

  $ git pull --tags --rebase 1> /dev/null

  $ git log --tags --graph --pretty="%d %s"
  *  (HEAD -> master, tag: tag_from_ws_1, origin/master) add in filter
  *  add file2
  *  add file1
  *  add file3
  *    Merge branch 'new1'
  |\  
  | *  (new1) add newfile1
  * |  newfile master
  |/  
  *  initial
  *  add workspace

# Pushing a tag from the workspace on an older commit

  $ cd ${TESTTMP}/ws
  $ git checkout HEAD~1 2>/dev/null
  $ git log --oneline
  6be0d68 add file2
  833812f add file1
  1b46698 add workspace
  $ git tag tag_from_ws_2
  $ git push origin tag_from_ws_2 -o base=refs/heads/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new tag]         JOSH_PUSH -> tag_from_ws_2
  remote: REWRITE(6be0d68b8e87001c8b91281636e21d6387010332 -> 69a79aa033a24fbddd9218c865eebdacef8bcd3d)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new tag]         tag_from_ws_2 -> tag_from_ws_2

  $ cd ${TESTTMP}/real_repo

  $ git pull --tags --rebase 1> /dev/null
  From http://localhost:8001/real_repo
   * [new tag]         tag_from_ws_2 -> tag_from_ws_2

  $ git log --tags --graph --pretty="%d %s"
  *  (HEAD -> master, tag: tag_from_ws_1, origin/master) add in filter
  *  add file2
  *  add file1
  *  add file3
  *    Merge branch 'new1'
  |\  
  | *  (new1) add newfile1
  * |  newfile master
  |/  
  *  initial
  *  add workspace
  *  (tag: tag_from_ws_2) add file2
  *  add file1
  *  add workspace

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
  |   |       `-- %3Aworkspace=ws
  |   |           `-- heads
  |   |               `-- master
  |   |-- rewrites
  |   |   `-- real_repo.git
  |   |       `-- 7bd92d97e96693ea7fd7eb5757b3580002889948
  |   |           |-- r_2cbcd105ead63a4fecf486b949db7f44710300e5
  |   |           `-- r_69a79aa033a24fbddd9218c865eebdacef8bcd3d
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  25 directories, 9 files

$ cat ${TESTTMP}/josh-proxy.out
