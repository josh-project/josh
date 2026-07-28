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
 
 
  $ echo content1 > root_file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null
 
  $ git checkout -b new1
  Switched to a new branch 'new1'
  $ echo content > newfile1 1> /dev/null
  $ git add .
  $ git commit -m "add newfile1" 1> /dev/null
 
  $ git checkout master 1> /dev/null
  Switched to branch 'master'
  $ echo content > newfile_master 1> /dev/null
  $ git add .
  $ git commit -m "newfile master" 1> /dev/null
 
  $ git merge -q new1 --no-ff
 
 
  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null
 
  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null
 
  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > pre/a/b = :/sub2
  > pre/c = :/sub1
  > EOF
 
  $ git add ws
  $ git commit -m "add workspace" 1> /dev/null
 
  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null
 
  $ cat > ws/workspace.josh <<EOF
  > pre/a/b = :/sub2
  > pre/c = :/sub1
  > pre/d = :/sub3
  > EOF
 
  $ git add ws
  $ git commit -m "mod workspace" 1> /dev/null
 
  $ git log --graph --pretty=%s
  * mod workspace
  * add file3
  * add workspace
  * add file2
  * add file1
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial
 
 
  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master
 
  $ cd ${TESTTMP}
 
  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws:/pre.git ws
  $ cd ws
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- d
      `-- file3
  
  6 directories, 3 files
 
  $ git log --graph --pretty=%s
  *   mod workspace
  |\  
  | * add file3
  * add file2
  * add file1
 
  $ git checkout -q HEAD~1 1> /dev/null
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  `-- c
      `-- subsub
          `-- file1
  
  5 directories, 2 files
 
  $ git checkout HEAD~1 1> /dev/null
  Previous HEAD position was 2a03ad0 add file2
  HEAD is now at 02668d7 add file1
  $ tree
  .
  `-- c
      `-- subsub
          `-- file1
  
  3 directories, 1 file
 
  $ git checkout master 1> /dev/null
  Previous HEAD position was 02668d7 add file1
  Switched to branch 'master'
 
  $ echo newfile_1_contents > c/subsub/newfile_1
  $ git rm c/subsub/file1
  rm 'c/subsub/file1'
  $ echo newfile_2_contents > a/b/newfile_2
  $ echo ws_file_contents > ws_file
 
  $ git add .
 
  $ git commit -m "add in filter" 1> /dev/null
 
  $ git push 2> /dev/null
 
  $ cd ${TESTTMP}/real_repo
 
  $ git pull --rebase 1> /dev/null
  From http://localhost:8001/real_repo
     2b7018e..005d8d5  master     -> origin/master
 
  $ git clean -ffdx 1> /dev/null
 
  $ tree
  .
  |-- newfile1
  |-- newfile_master
  |-- root_file1
  |-- sub1
  |   `-- subsub
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  |-- sub3
  |   `-- file3
  `-- ws
      |-- pre
      |   `-- ws_file
      `-- workspace.josh
  
  7 directories, 9 files
  $ git log --graph --pretty=%s
  * add in filter
  * mod workspace
  * add file3
  * add workspace
  * add file2
  * add file1
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial
 
 
  $ git checkout -q HEAD~1 1> /dev/null
  $ git clean -ffdx 1> /dev/null
  $ tree
  .
  |-- newfile1
  |-- newfile_master
  |-- root_file1
  |-- sub1
  |   `-- subsub
  |       `-- file1
  |-- sub2
  |   `-- file2
  |-- sub3
  |   `-- file3
  `-- ws
      `-- workspace.josh
  
  6 directories, 7 files
  $ cat sub1/subsub/file1
  contents1
 
  $ git checkout HEAD~1 1> /dev/null
  Previous HEAD position was 2b7018e mod workspace
  HEAD is now at d038198 add file3
  $ git clean -ffdx 1> /dev/null
  $ tree
  .
  |-- newfile1
  |-- newfile_master
  |-- root_file1
  |-- sub1
  |   `-- subsub
  |       `-- file1
  |-- sub2
  |   `-- file2
  |-- sub3
  |   `-- file3
  `-- ws
      `-- workspace.josh
  
  6 directories, 7 files
 
 
  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub1/",
      "::sub1/subsub/",
      "::sub2/",
      "::sub3/",
      "::ws/",
      ":workspace=ws",
      ":workspace=ws:/pre",
  ]
  .
  |-- josh
  |   `-- cache
  |       `-- 32
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
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 2a
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 2b
  |   |   |   `-- 7018edda866b44c516fb04e839ea39700efab3
  |   |   |-- 2c
  |   |   |   `-- 019b4143b03630bb24119a53063028dd29278f
  |   |   |-- 2e
  |   |   |   `-- 3a8b1b949c599093a761dfe0c7b67d5cb2c379
  |   |   |-- 39
  |   |   |   `-- 5f7f4760aa877be41b269a500ff31ac1d269a0
  |   |   |-- 3b
  |   |   |   `-- f87f094d6ca4a2ce40589d77455058f62a7c90
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 46
  |   |   |   `-- 7711599b943110e1c7eb163bbf0405686ebf3d
  |   |   |-- 4c
  |   |   |   `-- febd0af63043a59aa060491d8ff6664e434b15
  |   |   |-- 53
  |   |   |   `-- e006435dc62586ba2a60d63068f58c84f98a17
  |   |   |-- 54
  |   |   |   `-- c9bf12801b1ef6d4868d31a788e06d9c54549d
  |   |   |-- 6e
  |   |   |   `-- 3bc46526a71f258d493af2e19c6ff83c2df4fd
  |   |   |-- 71
  |   |   |   `-- 23ddfb609a7c13f41dcb25ab6cdf28667244cc
  |   |   |-- 79
  |   |   |   `-- 388d184069e254a0db889a3df7cf5daba91603
  |   |   |-- 7f
  |   |   |   `-- ec045dbd23b0596f513ff06bda5ebd6ccb909f
  |   |   |-- 80
  |   |   |   `-- f0cf0a2d152a122aac5288107b5da8da3a1b23
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 87
  |   |   |   `-- 4e45d38fa7fc718bd3690dd5f559792350267c
  |   |   |-- 8b
  |   |   |   `-- bf1f6fa6b0a01b986db884beb2f434b641bf13
  |   |   |-- 96
  |   |   |   |-- 61805f4c2c58fbeb14f2e573c29100c6193b3b
  |   |   |   `-- 91d65eea514970d4d75afd2490e7f1581f9df1
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- d0
  |   |   |   `-- 38198d656db500dcd177e32f89395de5805715
  |   |   |-- d1
  |   |   |   `-- 8254e66bcbb678dc87313cbdb61c152964ac45
  |   |   |-- d9
  |   |   |   `-- b9f715dbeb38158273bce8cabd40e8b894a6da
  |   |   |-- e0
  |   |   |   `-- c616efb9f8811a592e92fa90d3fcb6a39027dc
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- f5
  |   |   |   `-- 386e2d5fba005c1589dcbd9735fa1896af637c
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
      |   |-- 04
      |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
      |   |-- 2c
      |   |   `-- fb80c68126f24a61b2778ce3f55d65d93f5e90
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 64
      |   |   `-- 6fd2c5bfe156d57ba03f62f2fe735ddbb74e22
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
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
      |       |-- pack-89f5d1c01530839eab589ceec8565f11ff4679a5.idx
      |       |-- pack-89f5d1c01530839eab589ceec8565f11ff4679a5.pack
      |       |-- pack-a75d3ca2392eb679cf1dc24a3496576475f5e849.idx
      |       |-- pack-a75d3ca2392eb679cf1dc24a3496576475f5e849.pack
      |       |-- pack-c455f6d9504526b7c14867a795a5b2e27610956a.idx
      |       `-- pack-c455f6d9504526b7c14867a795a5b2e27610956a.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  65 directories, 57 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
