  $ EXTRA_OPTS="--filter-prefix=:/data" . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ mkdir data
  $ cd data
  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ cat > tmpl_file <<EOF
  > param: {{ param_val }}
  > EOF

  $ git add tmpl_file
  $ git commit -m "add tmpl_file" 1> /dev/null

  $ git push --all
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ echo changes_contents > sub2/on_change
  $ git add sub2
  $ git commit -m "add on_change" 1> /dev/null

  $ git push origin HEAD:refs/changes/123/2
  To http://localhost:8001/real_repo.git
   * [new reference]   HEAD -> refs/changes/123/2

  $ cd ${TESTTMP}
  $ tree real_repo
  real_repo
  `-- data
      |-- sub1
      |   `-- file1
      |-- sub2
      |   |-- file2
      |   `-- on_change
      `-- tmpl_file
  
  3 directories, 4 files

  $ git clone http://localhost:8002/real_repo.git:/sub2.git
  Cloning into 'sub2'...
  $ tree sub2
  sub2
  `-- file2
  
  0 directories, 1 file
  $ curl -s http://localhost:8002/real_repo.git:/sub1.git?get=file1
  contents1
