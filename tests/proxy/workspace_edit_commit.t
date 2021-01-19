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

  $ mkdir sub4
  $ echo contents4 > sub4/file4
  $ git add sub4
  $ git commit -m "add file4" 1> /dev/null

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
  * add file4
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

  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * add workspace

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
  > EOF

  $ git commit -a -F - <<EOF
  > Add new folder
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF
  [master 47a4447] Add new folder
   1 file changed, 1 insertion(+)

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote:  To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote: REWRITE(47a44473cfa06e213dd7b178b86c0ed03f71e2bf -> deb37ff512d2a0374ec09fad893171631985bc93)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new reference]   HEAD -> refs/for/master

  $ cd ${TESTTMP}/remote/real_repo.git/

  $ git update-ref refs/changes/1/1 refs/for/master

  $ git update-ref -d refs/for/master

  $ cd ${TESTTMP}/ws

  $ git fetch -q http://localhost:8002/real_repo.git@refs/changes/1/1:workspace=ws.git && git checkout -q FETCH_HEAD

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
  > e = :/sub4
  > EOF

  $ git commit -aq --amend -F - <<EOF
  > Add new folders
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote:  To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote: REWRITE(f8c037fec9ba2a828e96fbd6ac979ea8605340a0 -> 09f5bd0c1a8927ab1f4ba38756c4d3f266fd24cf)
  remote:
  remote:
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new reference]   HEAD -> refs/for/master

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub1/subsub',
      ':/sub2',
      ':/sub3',
      ':/sub4',
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
  |   |       |-- %3A%2Fsub4
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
  |   |       |-- r_09f5bd0c1a8927ab1f4ba38756c4d3f266fd24cf
  |   |       `-- r_deb37ff512d2a0374ec09fad893171631985bc93
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               |-- changes
  |               |   `-- 1
  |               |       `-- 1
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  28 directories, 11 files
