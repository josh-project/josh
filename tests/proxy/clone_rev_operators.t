  $ export GIT_TREE_FMT='%(objectmode) %(objecttype) %(objectname) %(path)'
  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ echo contents1 > file
  $ git add file
  $ git commit -q -m "commit1"

  $ echo contents2 > file
  $ git add file
  $ git commit -q -m "commit2"

  $ echo contents3 > file
  $ git add file
  $ git commit -q -m "commit3"

  $ echo contents4 > file
  $ git add file
  $ git commit -q -m "commit4"

  $ git log --oneline --no-abbrev-commit
  c76092ddde20e1071d88491ae990b60b95b50d8a commit4
  53b7ffe9ba2366c6d317a97bd154f7507ce5e151 commit3
  27ee1d4576a83a7c50484b680ba68edbbc196662 commit2
  c4c0a059d8a45f4248f68115445a99e2a241aff8 commit1

  $ git push -q
  $ git clone -q 'http://localhost:8002/real_repo.git:rev(%3C27ee1d4576a83a7c50484b680ba68edbbc196662:prefix=subdir).git' clone-test

  $ git -C clone-test log --oneline
  111c99d commit4
  defc311 commit3
  5687af0 commit2
  3200ecb commit1

  $ git -C clone-test ls-tree --format "${GIT_TREE_FMT}" 3200ecb
  040000 tree 51ca6cbff0e8a61e440831239e1a7750517a608a subdir

  $ git -C clone-test ls-tree --format "${GIT_TREE_FMT}" 111c99d
  100644 blob 288746e9035732a1fe600ee331de94e70f9639cb file
