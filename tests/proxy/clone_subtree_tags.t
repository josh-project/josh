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
  [master (root-commit) *] add file1 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file1

  $ git tag a_tag

  $ echo contents1 > sub1/file12
  $ git add sub1
  $ git commit -m "add file12"
  [master *] add file12 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/file12


  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2"
  [master *] add file2 (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file2

  $ git describe --tags
  a_tag-2-* (glob)

  $ tree
  .
  |-- sub1
  |   |-- file1
  |   `-- file12
  `-- sub2
      `-- file2
  
  2 directories, 3 files

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

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git sub1

  $ cd sub1

  $ tree
  .
  |-- file1
  `-- file12
  
  0 directories, 2 files

  $ git log --graph --pretty=%s
  * add file12
  * add file1

  $ git describe --tags
  a_tag-1-* (glob)

  $ cat file1
  contents1

  $ git fetch http://localhost:8002/real_repo.git@refs/tags/a_tag:/sub1.git HEAD
  From http://localhost:8002/real_repo.git@refs/tags/a_tag:/sub1
   * branch            HEAD       -> FETCH_HEAD

  $ git checkout FETCH_HEAD 2> /dev/null

  $ tree
  .
  `-- file1
  
  0 directories, 1 file

  $ git log --graph --pretty=%s
  * add file1

  $ git describe --tags
  a_tag

  $ bash ${TESTDIR}/destroy_test_env.sh
  remote/scratch/refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3A%2Fsub2
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               |-- heads
  |               |   `-- master
  |               `-- tags
  |                   `-- a_tag
  |-- namespaces
  `-- tags
  
  15 directories, 4 files
$ cat ${TESTTMP}/josh-proxy.out | grep TAGS
