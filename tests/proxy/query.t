  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

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

  $ curl -s http://localhost:8002/real_repo.git:/sub1.git?get=file1
  contents1
  $ curl -s http://localhost:8002/real_repo.git?render=tmpl_file\&param_val=12345
  param: 12345
  $ curl -s http://localhost:8002/real_repo.git?get=sub1/file1
  contents1
  $ curl -s http://localhost:8002/real_repo.git@refs/changes/123/2:nop.git?get=sub2/on_change
  changes_contents
  $ curl -s http://localhost:8002/real_repo.git@refs/changes/123/2?get=sub2/on_change
  changes_contents
