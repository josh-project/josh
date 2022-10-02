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

  $ git add ws/file1
  $ git commit -m "add file1" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  $ cd ws
  $ tree
  .
  `-- file1
  
  0 directories, 1 file

Error: comment in the middle
  $ cat > workspace.josh <<EOF
  > # comment
  > #
  > 
  > 
  > a/b = :/sub2
  > # comment 2
  > c = :/sub1
  > EOF

  $ git add workspace.josh
  $ git commit -m "add workspace file" 1> /dev/null
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote: 
  remote: Can't apply "add workspace file" (4f70c9a0179b1cae80148572c8dfc3ba1f2d43a2)        
  remote: Invalid workspace:        
  remote: ----        
  remote:  --> 6:1        
  remote:   |        
  remote: 6 | # comment 2        
  remote:   | ^---        
  remote:   |        
  remote:   = expected EOI, filter_spec, or dst_path        
  remote: 
  remote: # comment        
  remote: #        
  remote: 
  remote: 
  remote: a/b = :/sub2        
  remote: # comment 2        
  remote: c = :/sub1        
  remote: 
  remote: ----        
  remote: 
  remote: 
  remote: error: hook declined to update refs/heads/master        
  To http://localhost:8002/real_repo.git:workspace=ws.git
   ! [remote rejected] master -> master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:workspace=ws.git'
  [1]

Error in filter
  $ cat > workspace.josh <<EOF
  > a/b = :b/sub2
  > c = :/sub1
  > EOF

  $ git add workspace.josh
  $ git commit -m "add workspace file" --amend 1> /dev/null
  $ git push
  remote: josh-proxy        
  remote: response from upstream:        
  remote: 
  remote: Can't apply "add workspace file" (74128cac082e518bc3ddec183bb11b16856406cd)        
  remote: Invalid workspace:        
  remote: ----        
  remote:  --> 1:9        
  remote:   |        
  remote: 1 | a/b = :b/sub2        
  remote:   |         ^---        
  remote:   |        
  remote:   = expected EOI, filter_group, filter_group_arg, filter_subdir, filter_nop, filter_presub, filter, or filter_noarg        
  remote: 
  remote: a/b = :b/sub2        
  remote: c = :/sub1        
  remote: 
  remote: ----        
  remote: 
  remote: 
  remote: error: hook declined to update refs/heads/master        
  To http://localhost:8002/real_repo.git:workspace=ws.git
   ! [remote rejected] master -> master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:workspace=ws.git'
  [1]

No match for filters
  $ cat > workspace.josh <<EOF
  > ::abc
  > a/b = :/b/c/*
  > c = ::sub/
  > test = :[
  >   ::test
  >   ::sub/
  >   test = :/test
  >   :/test:[
  >     ::test/
  >   ]
  > ]
  > EOF

  $ git add workspace.josh
  $ git commit -m "add workspace file" --amend 1> /dev/null
  $ git push origin HEAD:master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    5119a73..25943be  JOSH_PUSH -> master        
  remote: warnings:        
  remote: No match for "::abc"        
  remote: No match for "a/b = :/b/c/*"        
  remote: No match for "c/sub = :/sub"        
  remote: No match for "test/sub = :/sub"        
  remote: No match for "test = ::test"        
  remote: No match for "test/test = :/test:/"        
  remote: No match for "::test/test/"        
  remote: REWRITE(064643c5fdf5295695d383a511e4335ea3262fce -> 9cbc5874da793480ee59207ca72d9f0523b8b127)        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:workspace=ws.git
     66a8b5e..064643c  HEAD -> master

warnings with graphql
$ curl -s http://localhost:8002/flush
Flushed credential cache

  $ cat > ../query << EOF
  > {"query": "{
  >  rev(at:\"refs/heads/master\", filter:\":workspace=ws\") {
  >    warnings {
  >      message
  >    }
  >  }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "warnings": [
          {
            "message": "No match for \"::abc\""
          },
          {
            "message": "No match for \"a/b = :/b/c/*\""
          },
          {
            "message": "No match for \"c/sub = :/sub\""
          },
          {
            "message": "No match for \"test/sub = :/sub\""
          },
          {
            "message": "No match for \"test = ::test\""
          },
          {
            "message": "No match for \"test/test = :/test:/\""
          },
          {
            "message": "No match for \"::test/test/\""
          }
        ]
      }
    }
  } (no-eol)
$ cat ${TESTTMP}/josh-proxy.out
