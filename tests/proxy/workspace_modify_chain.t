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
      |   |-- 00
      |   |   `-- 5d8d52eafd3d5f0b2272be08b36693b2b5b76b
      |   |-- 02
      |   |   `-- 668d7af968c8eed910db7539a57b18dd62a50e
      |   |-- 04
      |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
      |   |-- 0c
      |   |   `-- d4309cc22b5903503a7196f49c24cf358a578a
      |   |-- 13
      |   |   `-- 10813ea4e1d46c4c5c59bfdaf97a6de3b24c31
      |   |-- 16
      |   |   `-- 2e9b45be8bbc12e975d13c0089eccbbcba9b56
      |   |-- 17
      |   |   `-- b82a3b31749dc7d6dbc5a528eb19a103a5bdb9
      |   |-- 1d
      |   |   |-- 6d81877130bc5a54c8ea27e8497f838cfe9aa3
      |   |   `-- ff8a2dce92f202d13c5995a0f898b6cfba26c9
      |   |-- 20
      |   |   `-- 5dccdc2e37df9aaccec46009520154219134e7
      |   |-- 27
      |   |   `-- d0eee16bdbe4a1ade0ebf877f97467de3b218e
      |   |-- 2a
      |   |   |-- 03ad0fe1720ee0afc95ba8e1bc38a35b87983f
      |   |   `-- 3e798288165d5b090a10460984776489bcc7cc
      |   |-- 2b
      |   |   `-- 158ac79b2dae332577be854a9fc8fb558f13f9
      |   |-- 2c
      |   |   `-- fb80c68126f24a61b2778ce3f55d65d93f5e90
      |   |-- 2d
      |   |   `-- b4c60cceb3afe11f40f945c01dd5cd513f5653
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 30
      |   |   `-- ab6d81f7ffe88ab3a82ff74ff09439b2d5afa7
      |   |-- 34
      |   |   `-- be7abf6de2d37711a568059e7aa150ed428a43
      |   |-- 39
      |   |   |-- 0caf88590ab9ce40b6092c82a5f5e68041e4a6
      |   |   `-- abfc68c47fd430cd9775fc18c9f93bc391052e
      |   |-- 43
      |   |   `-- 52611a9e7c56dfdfeadec043ced6d6ef7a5c33
      |   |-- 46
      |   |   `-- 3b7672b3bcf00bbd95e650076c375528d5f5c3
      |   |-- 47
      |   |   `-- 8644b35118f1d733b14cafb04c51e5b6579243
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 4f
      |   |   `-- 314491719dffbb4340268e5f04136e74821e2c
      |   |-- 53
      |   |   `-- 0148978e42a91926dbe2f5fe0c71fe63aacf8e
      |   |-- 5a
      |   |   `-- 38ed8e4d96b624c6ef7e36f8b696d52be785db
      |   |-- 5b
      |   |   `-- 545e12c5b297509b8d99df5f0b952a2dd7862d
      |   |-- 64
      |   |   |-- 6fd2c5bfe156d57ba03f62f2fe735ddbb74e22
      |   |   `-- d1f8d32b274d8c1eeb69891931f52b6ade9417
      |   |-- 67
      |   |   `-- 48a16f7d9b29607fcd4df93681361a6105a14a
      |   |-- 69
      |   |   `-- a612aea46e9d365755e3b930d8aa4458e7bbf6
      |   |-- 6f
      |   |   `-- 4ee6b7661cc1905bb762d80a94c4337d43697d
      |   |-- 70
      |   |   `-- 1b7746c347750d035ddd29ad67ad2f2c4851a8
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
      |   |-- 78
      |   |   `-- 2f6261fa32f8bfec7b89f77bb5cce40c4611cb
      |   |-- 7c
      |   |   `-- 30b7adfa79351301a11882adf49f438ec294f8
      |   |-- 7f
      |   |   `-- c8ee5474068055f7740240dfce6fa6e38bbf4d
      |   |-- 89
      |   |   `-- 8b763a1483259f4667f399a019b96f52a28f8c
      |   |-- 8a
      |   |   `-- bb8094049472170b402ad3aaee6db3d5a97286
      |   |-- 91
      |   |   `-- 584adddb4f190d805ec45ce500a0661671fb25
      |   |-- 93
      |   |   `-- f66d258b7b4c3757e63f985b08f7daa33db64e
      |   |-- 98
      |   |   `-- 84cc2efe368ea0aa9d912fa596b26c5d75dbee
      |   |-- 9d
      |   |   `-- e3bbb26e2b40f02ca8de195933eb620bbf0b6a
      |   |-- 9e
      |   |   `-- 4d2bcaee240904058a6160e84311667b409b08
      |   |-- 9f
      |   |   `-- 8daab1754f04fbe8aaac6fcbb44c8324df09eb
      |   |-- a3
      |   |   `-- d027b4cc082c08af95fcbe03eecd9a62a08c48
      |   |-- a7
      |   |   `-- 3af649c49fa4b0facff36fafdc1e2bef594d4e
      |   |-- aa
      |   |   `-- 0654329f0642e4505db87c24aea80ba52fd689
      |   |-- ac
      |   |   `-- 0969c65843463b2d0e86fd4c6efcae33012332
      |   |-- b0
      |   |   `-- 3b5fbc9a12109cd3f5308929e4812b3c998da6
      |   |-- b8
      |   |   `-- 54e8ec3db62174179b215404936a58f1bb6a79
      |   |-- b9
      |   |   `-- 90567474f1f8af9784478614483684e88ccf4f
      |   |-- bc
      |   |   |-- 61a0ff30ea25db7bcfc9a67fdae747904ed55f
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- c1
      |   |   `-- 489fc8fd6ae9ac08c0168d7cabaf5645b922fa
      |   |-- c2
      |   |   |-- 054840bbf17ac9939d8165a0b88f1065ff57f7
      |   |   `-- d86319b61f31a7f4f1bc89b8ea4356b60c4658
      |   |-- c5
      |   |   `-- 30c1385a6297aa79fd3d1ced94b089ca760fd6
      |   |-- cd
      |   |   |-- 0544cd5f702a9b3418205ec0425c6ae77f9e3e
      |   |   `-- 7c94c08f59302a2b1587a01d4fd1680d7378c9
      |   |-- d7
      |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |-- e1
      |   |   `-- 25e6d9f8f9acca5ffd25ee3c97d09748ad2a8b
      |   |-- e5
      |   |   `-- f5b3645e5400bd404016c5111c18c3942f02e7
      |   |-- e8
      |   |   `-- d34a664c80ff36cdff2c41c1fd3964f6e30f00
      |   |-- ea
      |   |   `-- 1ae75547e348b07cb28a721a06ef6580ff67f0
      |   |-- ec
      |   |   `-- 4f59ca1a0ac5b2f375d4917dbba5e6aedff12a
      |   |-- ef
      |   |   `-- 4639405a27655c750c3ca6249b35fbf3ea25ca
      |   |-- f0
      |   |   `-- 932972a6075f9a82e1ca23d65f9983145abd9e
      |   |-- f2
      |   |   |-- 257977b96d2272be155d6699046148e477e9fb
      |   |   `-- 7e0d18d976fd84da0a9e260989ecb6edaa593f
      |   |-- f6
      |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |-- f7
      |   |   `-- 7930069cbf71e47b72fb4e5ede3dff15123884
      |   |-- fa
      |   |   `-- 1745f6c84f945b51a305aa9751c466e26fb78a
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  122 directories, 117 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
