  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) bb282e9] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ git tag a_tag

  $ echo contents1 > sub1/file12
  $ git add sub1
  $ git commit -m "add file12"
  [master fa432ae] add file12
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file12

  $ git tag -m "a tag object" a_tag_object

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2"
  [master bbc3f80] add file2
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file2

  $ git describe --tags
  a_tag_object-1-gbbc3f80

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file12
  `-- sub2
      `-- file2
  
  3 directories, 3 files

  $ git log --graph --pretty=%s
  * add file2
  * add file12
  * add file1

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master


  $ git push --tags
  To http://localhost:8001/real_repo.git
   * [new tag]         a_tag -> a_tag
   * [new tag]         a_tag_object -> a_tag_object

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git sub1

  $ cd sub1

  $ tree
  .
  |-- file1
  `-- file12
  
  1 directory, 2 files

  $ git log --graph --pretty=%s
  * add file12
  * add file1

  $ git describe --tags
  a_tag_object

  $ cat file1
  contents1

  $ git fetch http://localhost:8002/real_repo.git@refs/tags/a_tag:/sub1.git HEAD
  From http://localhost:8002/real_repo.git@refs/tags/a_tag:/sub1
   * branch            HEAD       -> FETCH_HEAD

  $ git checkout FETCH_HEAD 2> /dev/null

  $ tree
  .
  `-- file1
  
  1 directory, 1 file

  $ git log --graph --pretty=%s
  * add file1

  $ git describe --tags
  a_tag

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ":/sub1",
      "::sub1/",
      "::sub2/",
  ]
  .
  |-- josh
  |   `-- 22
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
  |   |   |-- 17
  |   |   |   `-- 27e7d219402e1ce54587731575e941130d09ac
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 56
  |   |   |   `-- e190237f1fa5d07f52fc7de0e4b7d04128c79d
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   |-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |   |-- c3f8026800792a43ffbc932153f4864509378e
  |   |   |   `-- f54cff926d013ce65a3b1cf4e8d239c43beb4b
  |   |   |-- c1
  |   |   |   `-- 90f9e0d45065e20a13996f541c3571ed317c45
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- fa
  |   |   |   `-- 432ae56bb033f625197b126825c347ff557661
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               |-- heads
  |       |               |   `-- master
  |       |               `-- tags
  |       |                   |-- a_tag
  |       |                   `-- a_tag_object
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 0b
      |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
      |   |-- 6e
      |   |   `-- 99e1e5ba1de7225f0d09a0b91d2e29ae15569c
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  39 directories, 28 files
$ cat ${TESTTMP}/josh-proxy.out | grep TAGS
