  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents > sub1/test
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ cat > x.graphql <<EOF
  > query {
  >  hash
  >  rev(filter: "::**/file*")
  >  {
  >    hash
  >  }
  > }
  > EOF

  $ cat > tmpl_file <<EOF
  > param: {{ param_val }}
  > {{ #with (graphql file="x.graphql") as |gql| }}
  > sha: {{ gql.hash }}
  > filter_sha: {{gql.rev.hash}}
  > {{ /with }}
  > EOF

  $ git add x.graphql
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
Get works
  $ curl -s http://localhost:8002/real_repo.git:/sub1.git?get=file1
  contents1

Filter once before calling render
  $ git clone http://localhost:8002/real_repo.git::**/file*.git
  Cloning into 'file*'...

Now render still works (used to fail if filtered previously)
  $ curl -s http://localhost:8002/real_repo.git?render=tmpl_file\&param_val=12345
  param: 12345
  sha: 890148bbaa6a797bac8aef672a437f2b08635f15
  filter_sha: ffe8d082c1034053534ea8068f4205ac72a1098e

Graphql works
  $ curl -s http://localhost:8002/real_repo.git?graphql=x.graphql
  {
    "hash": "890148bbaa6a797bac8aef672a437f2b08635f15",
    "rev": {
      "hash": "ffe8d082c1034053534ea8068f4205ac72a1098e"
    }
  } (no-eol)


Failing render for lack of variable
  $ curl -i -s http://localhost:8002/real_repo.git?render=tmpl_file
  HTTP/1.1 422 Unprocessable Entity\r (esc)
  content-length: 112\r (esc)
  date: *\r (esc) (glob)
  \r (esc)
  JoshError(Error rendering "tmpl_file" line 1, col 8: Failed to access variable in strict mode Some("param_val")) (no-eol)



  $ curl -s http://localhost:8002/real_repo.git?get=sub1/file1
  contents1
  $ curl -s http://localhost:8002/real_repo.git@refs/changes/123/2:nop.git?get=sub2/on_change
  changes_contents
  $ curl -s http://localhost:8002/real_repo.git@refs/changes/123/2?get=sub2/on_change
  changes_contents
