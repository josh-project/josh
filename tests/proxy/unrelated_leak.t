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
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new branch]      JOSH_PUSH -> from_filtered
  To http://localhost:8002/real_repo.git:/sub1.git
   * [new branch]      master -> from_filtered

  $ git push origin master:refs/heads/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:    db0fd21..e170e96  JOSH_PUSH -> master
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
      ":/sub1",
      "::sub1/",
      "::sub2/",
  ]
  .
  |-- josh
  |   `-- 20
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
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 24
  |   |   |   `-- ce298ca12ec52cff187ef6638a2ba3d1c9503d
  |   |   |-- 25
  |   |   |   `-- 7cc5642cb1a054f08cc83f2d943e56fd3ebe99
  |   |   |-- 26
  |   |   |   `-- 1fdca7dd1cc67009c11551191e84aa1ba21c20
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 3e
  |   |   |   `-- b22065e1ba8caa4e5f20a9eaadd1803996dff5
  |   |   |-- 42
  |   |   |   `-- e0161c1ad82c05895e0f2caeae95925ac5ae6a
  |   |   |-- 66
  |   |   |   `-- 472b80301b889cf27a92d43fc2c2d8fbf4729d
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 86
  |   |   |   `-- 5c34e9a2c40198324cdc2fc796827653cb11df
  |   |   |-- 8a
  |   |   |   `-- f523d3883c24610cf99813ea15df65eb20ea84
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a1
  |   |   |   `-- 1885ec53fe483199d9515bf4662e5cf94d9a9e
  |   |   |-- c2
  |   |   |   `-- f224659ea69f54d3960b776237069bfbf2ed6e
  |   |   |-- d5
  |   |   |   `-- 26cba5e5dae29edfc16218060d56958081c453
  |   |   |-- db
  |   |   |   |-- 0fd21be0dea377057285e6119361753587f667
  |   |   |   `-- 184bf11ece7fbeb99395ab7676b80e7d98631a
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               `-- heads
  |       |                   |-- from_filtered
  |       |                   `-- master
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
      |   |-- 1c
      |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
      |   |-- 24
      |   |   `-- ce298ca12ec52cff187ef6638a2ba3d1c9503d
      |   |-- 26
      |   |   `-- 1fdca7dd1cc67009c11551191e84aa1ba21c20
      |   |-- 36
      |   |   `-- 8bfdc83325632d67cb93d96f095f0b04e4e26d
      |   |-- 37
      |   |   `-- fad4aaffb0ee24ab0ad6767701409bfbc52330
      |   |-- 3f
      |   |   `-- 7ab67d01db03914916161b51dbda1a4635f8d2
      |   |-- 42
      |   |   `-- e0161c1ad82c05895e0f2caeae95925ac5ae6a
      |   |-- 6b
      |   |   `-- 46faacade805991bcaea19382c9d941828ce80
      |   |-- 86
      |   |   `-- 5c34e9a2c40198324cdc2fc796827653cb11df
      |   |-- 8a
      |   |   `-- f523d3883c24610cf99813ea15df65eb20ea84
      |   |-- b7
      |   |   `-- 97411c60180287c8da2d26c325038c44fd78b0
      |   |-- c8
      |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
      |   |-- cd
      |   |   `-- e160edcb6f5ed11cc6d74e64bf06679420c10c
      |   |-- da
      |   |   `-- 0d1f304c4686b5ed11c682fa9c8d1544c49b96
      |   |-- db
      |   |   `-- 184bf11ece7fbeb99395ab7676b80e7d98631a
      |   |-- dc
      |   |   `-- 0dbab6030c0673ece5706b067e74ca8a573397
      |   |-- e0
      |   |   `-- 6f0e81ff3d5f8d60ba86bf4f99b47c8aa47654
      |   |-- e1
      |   |   `-- 70e962d0fb4b94a491a176a7f39a6207ada3e8
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  63 directories, 51 files
