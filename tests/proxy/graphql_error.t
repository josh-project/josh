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

  $ mkdir ws
  $ cat > ws/file1 <<EOF
  > content
  > EOF

  $ git add ws/*
  $ git commit -m "add file1" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ cat > ../query << EOF
  > {"query": "{
  >  rev(at:\"refs/heads/master\") {
  >    this_field_does_not_exist
  >  }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "errors": [
      {
        "message": "Unknown field \"this_field_does_not_exist\" on type \"Revision\"",
        "locations": [
          {
            "line": 1,
            "column": 35
          }
        ]
      }
    ]
  }

  $ cat > ../query_syntax_error << EOF
  > {"query": "{
  >  rev(at:\"refs/heads/master\") {
  >    hash
  > }"}
  > EOF

  $ cat ../query_syntax_error | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "errors": [
      {
        "message": "Unexpected end of input",
        "locations": [
          {
            "line": 1,
            "column": 40
          }
        ]
      }
    ]
  }

  $ cat > ../query_broken_json << EOF
  > {invalid json here
  > EOF

  $ cat ../query_broken_json | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  key must be a string at line 1 column 2 (no-eol)
