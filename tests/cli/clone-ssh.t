  $ export JOSH_TEST_SSH=1
  $ . ${TESTDIR}/../proxy/setup_test_env.sh

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

Test josh clone via SSH (no filter)

  $ josh clone ssh://git@127.0.0.1:9001/$(pwd)/repo1-bare.git repo1-clone-josh
  Successfully added remote 'origin' with filter ':/:prune=trivial-merge'
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin
  Successfully cloned repository to: repo1-clone-josh

  $ cd repo1-clone-josh
  $ ls
  subdir
  test1
  $ cat test1
  test
  $ cat subdir/test2
  test
  $ cd ..

Test josh clone via SSH (with filter)

  $ josh clone ssh://git@127.0.0.1:9001/$(pwd)/repo1-bare.git:/subdir repo1-clone-josh-filtered
  Successfully added remote 'origin' with filter ':/subdir:prune=trivial-merge'
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin
  Successfully cloned repository to: repo1-clone-josh-filtered

  $ cd repo1-clone-josh-filtered
  $ ls
  test2
  $ cat test2
  test
  $ cd ..

Test josh clone via SSH (with explicit filter argument)

  $ josh clone ssh://git@127.0.0.1:9001/$(pwd)/repo1-bare.git repo1-clone-josh-explicit --filter :/subdir
  Successfully added remote 'origin' with filter ':/subdir:prune=trivial-merge'
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin
  Successfully cloned repository to: repo1-clone-josh-explicit

  $ cd repo1-clone-josh-explicit
  $ ls
  test2
  $ cat test2
  test
  $ cd ..


