  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > EOF

  $ git add ws
  $ git commit -m "add workspace" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file2
  * add workspace


  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  $ cd ws
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  `-- workspace.josh
  
  3 directories, 2 files

  $ git log --graph --pretty=%s
  * add file2
  * add workspace

  $ git checkout master 1> /dev/null
  Already on 'master'

  $ mkdir -p c/subsub
  $ echo newfile_1_contents > c/subsub/newfile_1
  $ echo newfile_2_contents > a/b/newfile_2

  $ git add .

  $ git commit -m "add in filter" 1> /dev/null

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add .
  $ git commit -m "publish" 1> /dev/null

  $ git push 2> /dev/null

  $ cd ${TESTTMP}/real_repo

  $ git pull --rebase 1> /dev/null
  From http://localhost:8001/real_repo
     7ac8997..842d478  master     -> origin/master

  $ git clean -ffdx 1> /dev/null

  $ tree
  .
  |-- sub1
  |   `-- subsub
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  `-- ws
      `-- workspace.josh
  
  5 directories, 4 files
  $ git log --graph --pretty=%s
  * publish
  * add in filter
  * add file2
  * add workspace

  $ git checkout -q HEAD~1 1> /dev/null
  $ tree
  .
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  `-- ws
      |-- c
      |   `-- subsub
      |       `-- newfile_1
      `-- workspace.josh
  
  5 directories, 4 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub2/",
      "::ws/",
      ":workspace=ws",
  ]
  .
  |-- josh
  |   `-- cache
  |       `-- 28
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
  |   |   |-- 13
  |   |   |   `-- d9e121d4af98be6fc945b81d3c867172ade127
  |   |   |-- 2a
  |   |   |   `-- 9ac6425f7d937881422893fa4b9f6ee0cb9814
  |   |   |-- 7a
  |   |   |   `-- c89975da33b797feba305f5cc12bfb33b83c5d
  |   |   |-- 7b
  |   |   |   `-- 36ca25a7488f59e4f41c95567066fbf23bfb0e
  |   |   |-- 85
  |   |   |   |-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |   `-- edae8ccb9e64ebbf32249f228c9c0533ee9ffa
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- e1
  |   |   |   `-- 0c349e6060048d38d2949670b1160e0de87aa5
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
  |       |                   `-- master
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 00
      |   |   `-- 14c74ac19f876065ca26a4e9b578c2bc61ef18
      |   |-- 0f
      |   |   `-- 61d50b4e5a814afb71b3da5a2986efd37bc605
      |   |-- 20
      |   |   `-- c53646e34e6079f7dc3090be5c2c3fdd81a4f3
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
      |   |-- 77
      |   |   `-- b5962d8bd7ad507a02af9767c4cf68c0781200
      |   |-- 95
      |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
      |   |-- bc
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- d7
      |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |-- f2
      |   |   `-- 257977b96d2272be155d6699046148e477e9fb
      |   |-- f6
      |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |-- info
      |   `-- pack
      |       |-- pack-22feba8ae58a3472f764b3f5060292f7190e37fe.idx
      |       |-- pack-22feba8ae58a3472f764b3f5060292f7190e37fe.pack
      |       |-- pack-6b38b3f70def6c73711d6d00ddbf675f33f854b8.idx
      |       |-- pack-6b38b3f70def6c73711d6d00ddbf675f33f854b8.pack
      |       |-- pack-941b0ed2e56099a0daa91129b84275672d56b2be.idx
      |       `-- pack-941b0ed2e56099a0daa91129b84275672d56b2be.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  46 directories, 38 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
