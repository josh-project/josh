
  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd ${TESTTMP}/real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) bb282e9] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ export TESTPASS=$(curl -s http://localhost:8001/_make_user/testuser)

  $ curl -s "http://localhost:8002/~/graphql/real_repo.git?query=\{name\}"
  $ curl -s "http://testuser:wrongpass@localhost:8002/~/graphql/real_repo.git?query=\{name\}"
  $ curl -s "http://testuser:${TESTPASS}@localhost:8002/~/graphql/real_repo.git?query=\{name\}"
  {
    "data": {
      "name": "/real_repo"
    }
  } (no-eol)

  $ export URL=http://testuser:wrongpass@localhost:8002/~/graphql/real_repo.git
  $ curl -s -H "Content-Type: application/json" -X POST --data-binary @- ${URL} << EOF
  > {"query": "{ name }"}
  > EOF

  $ export URL=http://testuser:${TESTPASS}@localhost:8002/~/graphql/real_repo.git
  $ curl -s -H "Content-Type: application/json" -X POST --data-binary @- ${URL} << EOF
  > {"query": "{ name }"}
  > EOF
  {
    "data": {
      "name": "/real_repo"
    }
  } (no-eol)

  $ git clone -q http://testuser:wrongpass@localhost:8002/real_repo.git full_repo
  fatal: Authentication failed for 'http://localhost:8002/real_repo.git/'
  [128]

  $ rm -Rf full_repo

  $ git clone -q http://testuser:${TESTPASS}@localhost:8002/real_repo.git full_repo

  $ cd full_repo
  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ cat sub1/file1
  contents1

  $ echo contents1 > file2
  $ git add .
  $ git commit -m "push test"
  [master f23daa6] push test
   1 file changed, 1 insertion(+)
   create mode 100644 file2
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    bb282e9..f23daa6  JOSH_PUSH -> master        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git
     bb282e9..f23daa6  master -> master

  $ rm -Rf full_repo
  $ git clone -q http://x\':bla@localhost:8002/real_repo.git full_repo
  fatal: Authentication failed for 'http://localhost:8002/real_repo.git/'
  [128]
  $ tree
  .
  |-- file2
  `-- sub1
      `-- file1
  
  1 directory, 2 files

  $ cd ${TESTTMP}/real_repo
  $ curl -s http://localhost:8001/_noauth
  $ git pull --rebase 2> /dev/null
  Updating bb282e9..f23daa6
  Fast-forward
   file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 file2
  $ git log --graph --pretty=%s
  * push test
  * add file1

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub1']
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
