  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

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

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2"
  [master ffe8d08] add file2
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file2


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ curl -s http://localhost:8002/real_repo.git@refs/heads/master:/sub1.git?info
  {"commit":"ffe8d082c1034053534ea8068f4205ac72a1098e","tree":"2387c32648eefdee78386575672ac091da849b08","parents":[{"commit":"bb282e9cdc1b972fffd08fd21eead43bc0c83cb8","tree":"c82fc150c43f13cc56c0e9caeba01b58ec612022"}],"filtered":{"commit":"0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb","tree":"3d77ff51363c9825cc2a221fc0ba5a883a1a2c72","parents":[]}}

  $ curl -s http://localhost:8002/real_repo.git@refs/heads/master:/nothing_here.git?info
  {"commit":"ffe8d082c1034053534ea8068f4205ac72a1098e","tree":"2387c32648eefdee78386575672ac091da849b08","parents":[{"commit":"bb282e9cdc1b972fffd08fd21eead43bc0c83cb8","tree":"c82fc150c43f13cc56c0e9caeba01b58ec612022"}],"filtered":{"commit":"0000000000000000000000000000000000000000","tree":"0000000000000000000000000000000000000000","parents":[]}}

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub2',
  ]
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- HEAD
  |   |       `-- %3A%2Fsub2
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  `-- tags
  
  11 directories, 4 files
