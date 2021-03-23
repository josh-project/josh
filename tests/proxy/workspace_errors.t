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
  remote: 6 | # comment 2\xe2\x90\x8a         (esc)
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
  remote: 1 | a/b = :b/sub2\xe2\x90\x8a         (esc)
  remote:   |         ^---        
  remote:   |        
  remote:   = expected EOI, filter_group, filter_subdir, filter_nop, filter_presub, filter, or filter_noarg        
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

$ cat ${TESTTMP}/josh-proxy.out
