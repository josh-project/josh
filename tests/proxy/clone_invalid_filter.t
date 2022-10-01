  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.


  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1"
  [master (root-commit) 03dfdf5] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/subsub/file1

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2"
  [master 79e0ba4] add file2
   1 file changed, 1 insertion(+)
   create mode 100644 sub2/file2

  $ tree
  .
  |-- sub1
  |   `-- subsub
  |       `-- file1
  `-- sub2
      `-- file2
  
  3 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * add file1

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:sub1.git
  remote: Invalid filter: ":sub1"
  remote: 
  remote: Note: use forward slash at the start of the filter if you're
  remote: trying to select a subdirectory:
  remote: 
  remote:   :/sub1
  fatal: unable to access 'http://localhost:8002/real_repo.git:sub1.git/': The requested URL returned error: 500
  [128]

  $ git clone -q http://localhost:8002/real_repo.git:workspace.git
  remote: Filter ":workspace" requires an argument.
  remote: 
  remote: Note: use "=" to provide the argument value:
  remote: 
  remote:   :workspace=path
  remote: 
  remote: Where `path` is the path to the directory where workspace.josh file is located
  fatal: unable to access 'http://localhost:8002/real_repo.git:workspace.git/': The requested URL returned error: 500
  [128]

  $ git clone -q http://localhost:8002/real_repo.git:prefix.git
  remote: Filter ":prefix" requires an argument.
  remote: 
  remote: Note: use "=" to provide the argument value:
  remote: 
  remote:   :prefix=path
  remote: 
  remote: Where `path` is the path to be used as a prefix
  fatal: unable to access 'http://localhost:8002/real_repo.git:prefix.git/': The requested URL returned error: 500
  [128]

  $ git clone -q http://localhost:8002/real_repo.git:/subfolder
  remote: Invalid URL: "/real_repo.git:/subfolder"
  remote: 
  remote: Note: repository URLs should end with ".git":
  remote: 
  remote:   /real_repo.git:/subfolder.git
  fatal: unable to access 'http://localhost:8002/real_repo.git:/subfolder/': The requested URL returned error: 422
  [128]
