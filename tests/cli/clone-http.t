  $ . ${TESTDIR}/../proxy/setup_test_env.sh
  $ export RUST_LOG=error
  $ cd ${TESTTMP}

Create a test repo and push it to the existing real_repo.git

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.
  $ cd real_repo
  $ mkdir -p subdir
  $ echo test > test1
  $ echo test > subdir/test2
  $ git add test1 subdir/test2
  $ git commit -q -m "test"
  $ git push -q
  $ cd ..

Test josh clone via HTTP (no filter)

  $ josh clone http://127.0.0.1:8001/real_repo.git :/ repo1-clone-josh
  Added remote 'origin' with filter ':/:prune=trivial-merge'
  From http://127.0.0.1:8001/real_repo
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/repo1-clone-josh
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: repo1-clone-josh

  $ cd repo1-clone-josh
  $ ls
  subdir
  test1
  $ cat test1
  test
  $ cat subdir/test2
  test
  $ cd ..

Test josh clone via HTTP (with filter)

  $ josh clone http://127.0.0.1:8001/real_repo.git :/subdir repo1-clone-josh-filtered
  Added remote 'origin' with filter ':/subdir:prune=trivial-merge'
  From http://127.0.0.1:8001/real_repo
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/repo1-clone-josh-filtered
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: repo1-clone-josh-filtered

  $ cd repo1-clone-josh-filtered
  $ ls
  test2
  $ cat test2
  test
  $ cd ..

Test josh clone via HTTP (with explicit filter argument)

  $ josh clone http://127.0.0.1:8001/real_repo.git :/subdir repo1-clone-josh-explicit
  Added remote 'origin' with filter ':/subdir:prune=trivial-merge'
  From http://127.0.0.1:8001/real_repo
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/repo1-clone-josh-explicit
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: repo1-clone-josh-explicit

  $ cd repo1-clone-josh-explicit
  $ ls
  test2
  $ cat test2
  test
  $ cd ..


