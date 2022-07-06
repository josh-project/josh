  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/real_repo

  $ echo foo > bla
  $ git add .
  $ git commit -m "initial"
  [master (root-commit) 66472b8] initial
   1 file changed, 1 insertion(+)
   create mode 100644 bla

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:/sub1.git

  $ cd ${TESTTMP}/real_repo
  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add .
  $ git commit -m "unrelated on master" 1> /dev/null
  $ git push origin HEAD:refs/heads/master 1> /dev/null
  To http://localhost:8001/real_repo.git
     a11885e..db0fd21  HEAD -> master

  $ cd ${TESTTMP}/sub1
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git fetch

  $ echo contents2 > file4
  $ git add .
  $ git commit -m "add file4" 1> /dev/null

  $ echo contents3 > file4
  $ git add .
  $ git commit -m "edit file4" 1> /dev/null
  $ git push -o base=refs/heads/master origin master:refs/heads/from_filtered 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new branch]      JOSH_PUSH -> from_filtered
  remote:
  remote:
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      master -> from_filtered

  $ git push origin master:refs/heads/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy
  remote: response from upstream:
  remote: To http://localhost:8001/real_repo.git
  remote:    db0fd21..e170e96  JOSH_PUSH -> master
  remote:
  remote:
  To http://localhost:8002/real_repo.git:/sub1.git
     0b4cf6c..da0d1f3  master -> master

  $ cd ${TESTTMP}/real_repo
  $ git fetch
  From http://localhost:8001/real_repo
     db0fd21..e170e96  master        -> origin/master
   * [new branch]      from_filtered -> origin/from_filtered

  $ git log  origin/master
  commit e170e962d0fb4b94a491a176a7f39a6207ada3e8
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      edit file4
  
  commit 3f7ab67d01db03914916161b51dbda1a4635f8d2
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file4
  
  commit db0fd21be0dea377057285e6119361753587f667
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      unrelated on master
  
  commit a11885ec53fe483199d9515bf4662e5cf94d9a9e
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file1
  
  commit 66472b80301b889cf27a92d43fc2c2d8fbf4729d
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      initial
  $ git log origin/from_filtered
  commit 865c34e9a2c40198324cdc2fc796827653cb11df
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      edit file4
  
  commit 42e0161c1ad82c05895e0f2caeae95925ac5ae6a
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file4
  
  commit a11885ec53fe483199d9515bf4662e5cf94d9a9e
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file1
  
  commit 66472b80301b889cf27a92d43fc2c2d8fbf4729d
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      initial
  $ . ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub1',
      ':/sub2',
  ]
  refs
  |-- heads
  |-- josh
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   |-- from_filtered
  |                   `-- master
  |-- namespaces
  `-- tags
  
  8 directories, 3 files
