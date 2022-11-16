
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

  $ kill ${SSH_AGENT_PID}
