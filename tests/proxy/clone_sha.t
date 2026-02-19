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

  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file

  $ git push -q

  $ git switch -q -c branch-1
  $ echo contents2 >> sub1/file1
  $ git add sub1/file1
  $ git commit -q -m "edit file1"
  $ git push -q origin branch-1
  $ git switch -q master

  $ git show HEAD
  commit bb282e9cdc1b972fffd08fd21eead43bc0c83cb8
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file1
  
  diff --git a/sub1/file1 b/sub1/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub1/file1
  @@ -0,0 +1 @@
  +contents1

  $ cd ${TESTTMP}

Checks the following:

1) Two different formats for separating origin ref in the remote URL
2) Ensures that using @sha works
3) Ensures extra refs are not filtered, and only the requested ref is served

  $ git ls-remote http://localhost:8002/real_repo.git@bb282e9cdc1b972fffd08fd21eead43bc0c83cb8:/.git | tr '\t' ' '
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8 HEAD
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8 refs/heads/_bb282e9cdc1b972fffd08fd21eead43bc0c83cb8

  $ git ls-remote http://localhost:8002/real_repo.git^bb282e9cdc1b972fffd08fd21eead43bc0c83cb8:/.git | tr '\t' ' '
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8 HEAD
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8 refs/heads/_bb282e9cdc1b972fffd08fd21eead43bc0c83cb8

Check (2) and (3) but with a branch ref

  $ git ls-remote http://localhost:8002/real_repo.git^refs/heads/branch-1:/.git | tr '\t' ' '
  36c6ab9d481503e14a88f783e87f3791aa8cef99 HEAD
  36c6ab9d481503e14a88f783e87f3791aa8cef99 refs/heads/branch-1

  $ git clone -q http://localhost:8002/real_repo.git@bb282e9cdc1b972fffd08fd21eead43bc0c83cb8:/.git full_repo

  $ cd full_repo

  $ tree
  .
  `-- sub1
      `-- file1
  
  2 directories, 1 file

  $ cat sub1/file1
  contents1


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = ["::sub1/"]
  .
  |-- josh
  |   `-- cache
  |       `-- 26
  |           `-- sled
  |               |-- blobs
  |               |-- conf
  |               `-- db
  |-- mirror
  |   |-- FETCH_HEAD
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- 36
  |   |   |   `-- c6ab9d481503e14a88f783e87f3791aa8cef99
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 55
  |   |   |   `-- a6786cdd5f290477fefa01bb0916555193b005
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
  |   |   |-- d0
  |   |   |   `-- 6477a050061f481e7bdbbff347d513ed321c32
  |   |   |-- df
  |   |   |   `-- 698318ca7a8dbab2f16ae1d43f693fd6ff1262
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           |-- bb282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |       |           `-- refs
  |       |               `-- heads
  |       |                   |-- branch-1
  |       |                   `-- master
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  36 directories, 23 files

