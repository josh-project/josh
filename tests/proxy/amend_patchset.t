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

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file3
  * initial

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git full
  $ cd ${TESTTMP}/full
  $ tree
  .
  |-- file1
  `-- sub3
      `-- file3
  
  1 directory, 2 files

  $ git log --graph --pretty=%s
  * add file3
  * initial

  $ echo content2 > file_outside 1> /dev/null
  $ echo content3 > sub3/file2x 1> /dev/null
  $ git add .
  $ git commit -aq -F - <<EOF
  > Add in full
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote:
  remote:
  To http://localhost:8002/real_repo.git
   * [new reference]   HEAD -> refs/for/master

  $ cd ${TESTTMP}/remote/real_repo.git/
  $ git update-ref refs/changes/1/1 refs/for/master
  $ git update-ref -d refs/for/master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub3.git sub
  $ cd ${TESTTMP}/sub
  $ git fetch -q http://localhost:8002/real_repo.git@refs/changes/1/1:/sub3.git && git checkout -q FETCH_HEAD
  $ git log --graph --pretty=%s
  * Add in full
  * add file3
  $ tree
  .
  |-- file2x
  `-- file3
  
  0 directories, 2 files

  $ echo content4 > file_new 1> /dev/null
  $ git add .
  $ git commit --amend --no-edit -q
  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote:
  remote:
  To http://localhost:8002/real_repo.git:/sub3.git
   * [new reference]   HEAD -> refs/for/master

  $ cd ${TESTTMP}/real_repo
  $ git fetch -q http://localhost:8002/real_repo.git@refs/for/master:nop.git && git checkout -q FETCH_HEAD
  $ git log --graph --pretty=%s
  * Add in full
  * add file3
  * initial
  $ tree
  .
  |-- file1
  |-- file_outside
  `-- sub3
      |-- file2x
      |-- file3
      `-- file_new
  
  1 directory, 5 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub3']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub3
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3Anop
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               |-- changes
  |               |   `-- 1
  |               |       `-- 1
  |               |-- for
  |               |   `-- master
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  17 directories, 5 files
