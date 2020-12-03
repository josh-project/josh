  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file1

  $ tree
  .
  `-- sub1
      `-- file1
  
  1 directory, 1 file

  $ git push origin master
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git filter
  $ cd filter
  $ git log --graph --pretty=%s master
  * add file1
  $ echo contents2 >> file1
  $ git branch newBranch
  $ git commit -am "update file1 from filter" 1> /dev/null
  $ git push origin HEAD:refs/heads/master1
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      JOSH_PUSH -> master1        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      HEAD -> master1

  $ cd ${TESTTMP}/real_repo
  $ echo contents1 >> unrelated_file
  $ git add unrelated_file
  $ git commit -am "unrelated change" 1> /dev/null
  $ echo contentsn >> sub1/file1
  $ git commit -am "related change" 1> /dev/null
  $ git log --graph --pretty=%s
  * related change
  * unrelated change
  * add file1
  $ git push origin HEAD:refs/heads/master3
  To http://localhost:8001/real_repo.git
   * [new branch]      HEAD -> master3

  $ cd ${TESTTMP}/filter
  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git fetch --all
  Fetching origin
  From http://localhost:8002/real_repo.git:/sub1
   * [new branch]      master3    -> origin/master3
  $ git checkout newBranch
  Switched to branch 'newBranch'
  $ echo contents3 >> file1
  $ git commit -a -m "commit" 1> /dev/null
  $ git push origin HEAD:refs/heads/master2
  remote: josh-proxy        
  remote: response from upstream:        
  remote:  To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      JOSH_PUSH -> master2        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      HEAD -> master2

  $ cd ${TESTTMP}/real_repo
  $ git fetch --all
  Fetching origin
  From http://localhost:8001/real_repo
   * [new branch]      master1    -> origin/master1
   * [new branch]      master2    -> origin/master2
  $ git log --graph --pretty=%s origin/master1
  * update file1 from filter
  * add file1
  $ git log --graph --pretty=%s origin/master2
  * commit
  * add file1
  $ git log --graph --pretty=%s origin/master3
  * related change
  * unrelated change
  * add file1


