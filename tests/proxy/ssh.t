
  $ export JOSH_TEST_SSH=1
  $ . ${TESTDIR}/setup_test_env.sh

  $ export GIT_SSH_COMMAND="ssh -o LogLevel=ERROR -o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no -o PreferredAuthentications=publickey -o ForwardAgent=no"

Create a bare repo where we will push

  $ mkdir repo1-bare.git
  $ cd repo1-bare.git
  $ git init -q --bare
  $ cd ..

Create a test repo and push it to bare repo on filesystem

  $ mkdir repo1
  $ cd repo1
  $ git init -q
  $ mkdir -p subdir
  $ echo test > test1
  $ echo test > subdir/test2
  $ git add test1 subdir/test2
  $ git commit -q -m "test"
  $ git remote add origin $(pwd)/../repo1-bare.git
  $ git push -q origin master
  $ cd ..

Clone from the "upstream" SSH server. SSH occasionally fails in docker when using
"localhost", so use loopback IP instead

  $ git clone -q ssh://git@127.0.0.1:9002/$(pwd)/repo1-bare.git repo1-clone-upstream
  $ ls repo1-clone-upstream
  subdir
  test1

Clone from josh

  $ git clone -q ssh://git@127.0.0.1:9001/$(pwd)/repo1-bare.git repo1-clone-josh1
  $ ls repo1-clone-josh1
  subdir
  test1

Clone from josh (with filter)

  $ git clone -q ssh://git@127.0.0.1:9001/$(pwd)/repo1-bare.git':[:/subdir].git' repo1-clone-josh2
  $ ls repo1-clone-josh2
  test2
  $ cat repo1-clone-josh2/test2
  test

Change contents on main branch

  $ echo "changed data on main" > repo1-clone-josh2/test2
  $ git -C repo1-clone-josh2 add test2
  $ git -C repo1-clone-josh2 commit -q -m "changed data on main"

Push over josh + ssh

  $ git -C repo1-clone-josh2 push origin master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To * (glob)
  remote:    19f34e8..44c0713  JOSH_PUSH -> master        
  To * (glob)
     f1a7421..44acd5a  master -> master

Check pushed contents on upstream

  $ git -C repo1-bare.git show refs/heads/master:subdir/test2
  changed data on main

Create a new branch from master

  $ git -C repo1-clone-josh2 switch -q -c new-branch
  $ echo "changed data on new-branch" > repo1-clone-josh2/test2
  $ git -C repo1-clone-josh2 add test2
  $ git -C repo1-clone-josh2 commit -q -m "changed data on new-branch"
  $ git -C repo1-clone-josh2 push -q origin new-branch -o base=master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To */repo1-bare.git* (glob)
  remote:  * [new branch]      JOSH_PUSH -> new-branch        

Check history of the new branch

  $ git -C repo1-bare.git log --oneline --graph refs/heads/new-branch
  * 070bb54 changed data on new-branch
  * 44c0713 changed data on main
  * 19f34e8 test

Push again: shouldn't need base option anymore

  $ echo "again for push without base" > repo1-clone-josh2/test2
  $ git -C repo1-clone-josh2 add test2
  $ git -C repo1-clone-josh2 commit -q -m "again for push without base"
  $ git -C repo1-clone-josh2 push origin new-branch
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To */repo1-bare.git* (glob)
  remote:    070bb54..6ab23f4  JOSH_PUSH -> new-branch        
  To */repo1-bare.git:[:/subdir].git (glob)
     f4efcfa..bbabdd5  new-branch -> new-branch

Amend a commit and force push from the new branch

  $ echo "changed data to prepare for force-push" > repo1-clone-josh2/test2
  $ git -C repo1-clone-josh2 add test2
  $ git -C repo1-clone-josh2 commit -q --amend --no-edit
  $ git -C repo1-clone-josh2 push -q origin new-branch -f -o base=master -o force
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To */repo1-bare.git* (glob)
  remote:  + 6ab23f4...29f0344 JOSH_PUSH -> new-branch (forced update)        
  remote: REWRITE(7e66aa3858a63747a5ee43619505b739022d1c9c -> 2770d6208c0575bf5a4769bac0040a68335f1284)        

Kill ssh-agent

  $ kill ${SSH_AGENT_PID}
