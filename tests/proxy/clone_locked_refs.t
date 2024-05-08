  $ export JOSH_META_REPO=/meta_repo.git
  $ . ${TESTDIR}/setup_test_env.sh

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/meta_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd meta_repo
  $ mkdir -p path/to/my_repo.git
  $ mkdir -p locked.git

  $ cat > path/to/my_repo.git/config.yml <<EOF
  > repo: /real_repo.git
  > filter: :prefix=my_prefix
  > EOF

  $ cat > locked.git/config.yml <<EOF
  > repo: /real_repo.git
  > filter: :prefix=my_prefix
  > lock_refs: true
  > EOF

  $ cat > locked.git/lock.yml <<EOF
  > refs/heads/master: fb5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a
  > EOF

  $ git add .
  $ git commit -m "add my_repo"
  [master (root-commit) bb41141] add my_repo
   3 files changed, 6 insertions(+)
   create mode 100644 locked.git/config.yml
   create mode 100644 locked.git/lock.yml
   create mode 100644 path/to/my_repo.git/config.yml
  $ git push
  To http://localhost:8001/meta_repo.git
   * [new branch]      master -> master


  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git backing_repo
  warning: You appear to have cloned an empty repository.

  $ cd backing_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) bb282e9] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2"
  [master ffe8d08] add file2
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file2


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ git ls-remote --symref http://localhost:8002/path/to/my_repo.git
  ref: refs/heads/master\tHEAD (esc)
  fb5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a\tHEAD (esc)
  fb5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a\trefs/heads/master (esc)
  $ git ls-remote --symref http://localhost:8002/locked.git
  ref: refs/heads/master\tHEAD (esc)
  fb5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a\tHEAD (esc)
  fb5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a\trefs/heads/master (esc)

  $ mkdir sub3
  $ echo contents1 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3"
  [master 791540d] add file3
   1 file changed, 1 insertion(+)
   create mode 100644 sub3/file3

  $ git push
  To http://localhost:8001/real_repo.git
     ffe8d08..791540d  master -> master

  $ git ls-remote --symref http://localhost:8002/path/to/my_repo.git
  ref: refs/heads/master\tHEAD (esc)
  99fe43f7d4c35346d3f70a65113862cadca00b55\tHEAD (esc)
  99fe43f7d4c35346d3f70a65113862cadca00b55\trefs/heads/master (esc)
  $ git ls-remote --symref http://localhost:8002/locked.git
  ref: refs/heads/master\tHEAD (esc)
  fb5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a\tHEAD (esc)
  fb5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a\trefs/heads/master (esc)



  $ bash ${TESTDIR}/destroy_test_env.sh
  "meta_repo.git" = [
      "::locked.git/",
      "::path/",
      "::path/to/",
  ]
  "real_repo.git" = [
      "::sub1/",
      "::sub2/",
      "::sub3/",
      ":prefix=my_prefix",
  ]
  .
  |-- josh
  |   `-- 19
  |       `-- sled
  |           |-- blobs
  |           |-- conf
  |           `-- db
  |-- mirror
  |   |-- FETCH_HEAD
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- 09
  |   |   |   `-- f7c8d30e284cb3cead2d6373d8de5ed35aebf1
  |   |   |-- 0a
  |   |   |   |-- 9d66d1a72a81d970c00ddb850b2335c97203f1
  |   |   |   `-- f71c71e5dbe48057a98aef239383a208de429a
  |   |   |-- 0d
  |   |   |   `-- d0327bf3dda29d1ca87d64b4913431f1557110
  |   |   |-- 23
  |   |   |   `-- 87c32648eefdee78386575672ac091da849b08
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 72
  |   |   |   `-- 16fe2a3cf2cb2ae5d069477709a71d1954c26b
  |   |   |-- 73
  |   |   |   `-- e02b9ab07380d56f29e97850bc2c874be4483d
  |   |   |-- 79
  |   |   |   `-- 1540d84292a90b8822f9d5ce6bdd88a0077aae
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a2
  |   |   |   `-- 75a2c5c7869aa5e8aed11360c715cdf39b014a
  |   |   |-- a6
  |   |   |   `-- b38a805ee48896f37a9eb5b0a1bac52c2a8009
  |   |   |-- a8
  |   |   |   `-- 789dddcb85c29e0e32e451cbd4f864a93420fd
  |   |   |-- bb
  |   |   |   |-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |   `-- 4114134b60eedbb957cd7b21c1b2de1dd411aa
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- e9
  |   |   |   `-- 077ac43b61a8e995793c9ce60c75e28639ccfd
  |   |   |-- ff
  |   |   |   `-- e8d082c1034053534ea8068f4205ac72a1098e
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       |-- meta_repo.git
  |       |       |   |-- HEAD
  |       |       |   `-- refs
  |       |       |       `-- heads
  |       |       |           `-- master
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               `-- heads
  |       |                   `-- master
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 5f
      |   |   `-- d3fa6f1f9fae3965027f948f68c7c1919bab22
      |   |-- 7f
      |   |   `-- 2b734e48e880677a729f0058869ac0fb61dbdd
      |   |-- 99
      |   |   `-- fe43f7d4c35346d3f70a65113862cadca00b55
      |   |-- a8
      |   |   `-- 53d3b05291c3f530657aa935b51d11369479ae
      |   |-- b5
      |   |   `-- ec4ca5a3d90abd48a9ee076b8bf195362f6327
      |   |-- fb
      |   |   `-- 5c5f4ad703335fd5d27b1bd51dc4a4c1f7211a
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  53 directories, 40 files
