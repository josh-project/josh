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
  
  1 directory, 1 file

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

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

  $ git ls-remote http://localhost:8002/real_repo.git@bb282e9cdc1b972fffd08fd21eead43bc0c83cb8:/.git 
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\tHEAD (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\trefs/heads/_bb282e9cdc1b972fffd08fd21eead43bc0c83cb8 (esc)

  $ git clone -q http://localhost:8002/real_repo.git@bb282e9cdc1b972fffd08fd21eead43bc0c83cb8:/.git full_repo

  $ cd full_repo

  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ cat sub1/file1
  contents1


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub1']
  refs
  |-- heads
  |-- josh
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           |-- bb282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  8 directories, 3 files

