  $ . ${TESTDIR}/../proxy/setup_test_env.sh
  $ export RUST_LOG=error
  $ cd ${TESTTMP}
  $ export JOSH_STORE=${TESTTMP}/store


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

  $ josh clone http://127.0.0.1:8001/real_repo.git repo1-clone-josh 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added remote 'real_repo' at 'real_repo'
  Cloned worktree to ${TESTTMP}/repo1-clone-josh

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

  $ josh clone http://127.0.0.1:8001/real_repo.git repo1-clone-josh-filtered --filter :/subdir 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Cloned worktree to ${TESTTMP}/repo1-clone-josh-filtered

  $ cd repo1-clone-josh-filtered
  $ ls
  test2
  $ cat test2
  test
  $ cd ..

Test josh clone via HTTP (with an explicit mount)

  $ josh clone http://127.0.0.1:8001/real_repo.git repo1-clone-josh-explicit --filter :/subdir --mount mirror/real_repo 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added remote 'mirror%2Freal_repo' at 'mirror/real_repo'
  Cloned worktree to ${TESTTMP}/repo1-clone-josh-explicit

  $ cd repo1-clone-josh-explicit
  $ ls
  test2
  $ cat test2
  test
  $ cd ..


