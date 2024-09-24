  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.


  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -q -b master


  $ echo content1 > file1 1> /dev/null
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ git checkout -q -b new1
  $ echo content > newfile1 1> /dev/null
  $ git add .
  $ git commit -m "add newfile1" 1> /dev/null

  $ git checkout -q master 1> /dev/null
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
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add ws
  $ git commit -m "add workspace" 1> /dev/null

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
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

  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws:prefix=pre:/pre.git ws
  $ cd ws
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  |-- d
  |   `-- file3
  `-- workspace.josh
  
  6 directories, 4 files

  $ git log --graph --pretty=%s
  *   mod workspace
  |\  
  | * add file3
  * add workspace
  * add file2
  * add file1

  $ git checkout -q HEAD~1 1> /dev/null
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  5 directories, 3 files

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

  $ git checkout -q master 1> /dev/null

  $ echo newfile_1_contents > c/subsub/newfile_1
  $ git rm c/subsub/file1
  rm 'c/subsub/file1'
  $ echo newfile_2_contents > a/b/newfile_2
  $ echo ws_file_contents > ws_file

  $ git add .

  $ git commit -m "add in filter" 1> /dev/null

  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:    aaec05d..edefd7d  JOSH_PUSH -> master
  remote: REWRITE(7de033196d3f74f40139647122f499286a97498b -> 44edc62d506b9805a3edfc74db15b1cc0bfc6871)
  To http://localhost:8002/real_repo.git:workspace=ws:prefix=pre:/pre.git
     6712cb1..7de0331  master -> master

  $ git pull -q origin master --rebase 1>/dev/null

  $ git mv d w
  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > w = :/sub3
  > EOF

  $ git add .
  $ git commit -m "try to modify ws" 1> /dev/null

  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:    edefd7d..0c66ddc  JOSH_PUSH -> master
  remote: REWRITE(9f7fe44ebf4b96d3fc03aa7bffff6baa4a84eb63 -> 707a20731ff94c2dee063a8b274665b1cc730e26)
  To http://localhost:8002/real_repo.git:workspace=ws:prefix=pre:/pre.git
     44edc62..9f7fe44  master -> master
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase 2> /dev/null

Note that d/ is still in the tree but now it is not overlayed
  $ tree
  .
  |-- a
  |   `-- b
  |       |-- file2
  |       `-- newfile_2
  |-- c
  |   `-- subsub
  |       `-- newfile_1
  |-- w
  |   `-- file3
  |-- workspace.josh
  `-- ws_file
  
  6 directories, 6 files



  $ cd ${TESTTMP}/real_repo

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase 1> /dev/null
  From http://localhost:8001/real_repo
     aaec05d..0c66ddc  master     -> origin/master

  $ git clean -ffdx 1> /dev/null

Note that ws/d/ is now present in the ws
  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   `-- subsub
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  |-- sub3
  |   `-- file3
  `-- ws
      |-- workspace.josh
      `-- ws_file
  
  6 directories, 9 files
  $ git log --graph --pretty=%s
  * try to modify ws
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

  $ cat sub1/subsub/file1
  *: No such file or directory (glob)
  [1]

  $ git checkout -q HEAD~1 1> /dev/null
  $ git clean -ffdx 1> /dev/null
  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   `-- subsub
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  |-- sub3
  |   `-- file3
  `-- ws
      |-- workspace.josh
      `-- ws_file
  
  6 directories, 9 files

  $ git checkout -q HEAD~1 1> /dev/null
  $ git clean -ffdx 1> /dev/null
  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
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
  |   |   |-- 01
  |   |   |   `-- 1335f884d15a84a6113337bb30a0be95c39fb9
  |   |   |-- 04
  |   |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
  |   |   |-- 0c
  |   |   |   `-- 66ddcaed3f256f7dacc400a684aa1b91ac638f
  |   |   |-- 0f
  |   |   |   `-- 7ceed53e5b4ab96efad3c0b77e2c00d10169ba
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 1e
  |   |   |   `-- 6ea69c6325d02f1dbc9614935f88ce9d2afbac
  |   |   |-- 2a
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 2c
  |   |   |   `-- 50404f5c69295bd3d4d0cb5475be9cc2aada23
  |   |   |-- 2f
  |   |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
  |   |   |-- 33
  |   |   |   `-- dcdc06e9d605c8aca2375b96f7d431d2eb41d7
  |   |   |-- 34
  |   |   |   `-- c24765275d6f3ec5d6baeaaa4299471d6f7df0
  |   |   |-- 36
  |   |   |   `-- 52f9baa44258d0f505314830ad37d16eafc981
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 41
  |   |   |   `-- 8fcc975168e0bfc9dd53bbb98f740da2e983c0
  |   |   |-- 48
  |   |   |   `-- a2132905aa1413bc0ac9762b4365c9222911c5
  |   |   |-- 53
  |   |   |   `-- 9f411b73b3c22bc218bece495a841880fd4e2c
  |   |   |-- 58
  |   |   |   `-- b0c1e483109b33f416e0ae08487b4d1b6bfd5b
  |   |   |-- 5d
  |   |   |   `-- 605cee0c66b1c25a15a2d435b2786cc0bc24c5
  |   |   |-- 6d
  |   |   |   `-- 4b5c23a94a89c7f26266ccf635647fd4002b19
  |   |   |-- 75
  |   |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
  |   |   |-- 7a
  |   |   |   `-- c71a2d1648e7de21f4fbe4935cf54b44bfef9a
  |   |   |-- 7c
  |   |   |   `-- 5a3be33ee5b7e18364f041b05de8ac08bf82ee
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 8a
  |   |   |   `-- 7fb63d6ac5e60e16941591969c5f8a8d23b8a5
  |   |   |-- 95
  |   |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a5
  |   |   |   `-- bc2cb1497c5491656a72647f07791fe11f4d8f
  |   |   |-- aa
  |   |   |   `-- ec05db5b89383100d4b35b6cf56c0bf36fa224
  |   |   |-- ad
  |   |   |   |-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |   `-- b5eed79dabe4a141b3e336a64bc1ea6ae70396
  |   |   |-- b7
  |   |   |   `-- 85a0b60f6ef7044b4c59c318e18e2c47686085
  |   |   |-- bc
  |   |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
  |   |   |-- c6
  |   |   |   `-- 6fb92e3be8e4dc4c89f94d796f3a4b1833e0fa
  |   |   |-- d7
  |   |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
  |   |   |-- e4
  |   |   |   `-- 5f0325cd9fab82d962b758e556d9bf8079fc37
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- eb
  |   |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
  |   |   |-- ed
  |   |   |   `-- efd7dc70381a72b7af5bff2e49b8eb60cb9237
  |   |   |-- ef
  |   |   |   `-- a3db3aed4c83fdd92ec9f72845df8898839fdd
  |   |   |-- f5
  |   |   |   `-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |-- f6
  |   |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
  |   |   |-- f8
  |   |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
  |   |   |-- fd
  |   |   |   `-- 2bc852c86f084dd411054c9c297b05ccf76427
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
      |   |-- 02
      |   |   `-- 668d7af968c8eed910db7539a57b18dd62a50e
      |   |-- 04
      |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
      |   |-- 09
      |   |   `-- b613e901acc0b5e4279ec6732261134a06f667
      |   |-- 0c
      |   |   |-- 66ddcaed3f256f7dacc400a684aa1b91ac638f
      |   |   `-- d4309cc22b5903503a7196f49c24cf358a578a
      |   |-- 13
      |   |   `-- 10813ea4e1d46c4c5c59bfdaf97a6de3b24c31
      |   |-- 14
      |   |   `-- da1560586adda328cca1fbf58c026d6730444f
      |   |-- 17
      |   |   `-- ac72002199728c133087acfa6b23009e00a52a
      |   |-- 19
      |   |   `-- b9637ac9437ea11d42632cd65ca2313952c32f
      |   |-- 20
      |   |   `-- 189c97a0ef53368b7ec335baa7a1b86bd76f8e
      |   |-- 22
      |   |   `-- b3eaf7b374287220ac787fd2bce5958b69115c
      |   |-- 23
      |   |   `-- d20398ffe09015ada5794763b97c750b61bdc4
      |   |-- 27
      |   |   `-- 5b45aec0a1c944c3a4c71cc71ee08d0c9ea347
      |   |-- 2a
      |   |   |-- 03ad0fe1720ee0afc95ba8e1bc38a35b87983f
      |   |   `-- 3e798288165d5b090a10460984776489bcc7cc
      |   |-- 2c
      |   |   |-- 50404f5c69295bd3d4d0cb5475be9cc2aada23
      |   |   `-- 913262d99f8cd15ea711a89e943cd902fb87a0
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 30
      |   |   `-- 48804b01e298df4a6e1bc60a1e3b2ca0b016bd
      |   |-- 31
      |   |   `-- af3d0a5be6cc36a10a6b984673087c2d068432
      |   |-- 34
      |   |   `-- c24765275d6f3ec5d6baeaaa4299471d6f7df0
      |   |-- 36
      |   |   `-- 52f9baa44258d0f505314830ad37d16eafc981
      |   |-- 39
      |   |   `-- abfc68c47fd430cd9775fc18c9f93bc391052e
      |   |-- 3a
      |   |   `-- 748f0be2a5670c0c282196d3a66620e8599ee5
      |   |-- 40
      |   |   `-- c389b6b248e13f3cb88dcd79467d7396a4489e
      |   |-- 43
      |   |   `-- 52611a9e7c56dfdfeadec043ced6d6ef7a5c33
      |   |-- 44
      |   |   |-- 625a9b34b1c6747c29903c3e641a4b2e580673
      |   |   `-- edc62d506b9805a3edfc74db15b1cc0bfc6871
      |   |-- 46
      |   |   `-- bc1eabe4b2029b9fcb661f0d447d8389d17337
      |   |-- 47
      |   |   `-- 8644b35118f1d733b14cafb04c51e5b6579243
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 4c
      |   |   `-- a9fbf65ee12663ac24db4be4cab10e53d6d6e7
      |   |-- 5a
      |   |   `-- 8a4b8c1452d54f1ca88454067369112809a4d2
      |   |-- 5b
      |   |   `-- 560252b0c3c6abc5e0327596a49efb15e494cb
      |   |-- 5e
      |   |   `-- 9213824faf1eb95d0c522329518489196374fd
      |   |-- 64
      |   |   |-- 6fd2c5bfe156d57ba03f62f2fe735ddbb74e22
      |   |   `-- d1f8d32b274d8c1eeb69891931f52b6ade9417
      |   |-- 67
      |   |   `-- 12cb1b8c89e3b2272182f140c81aef3b718671
      |   |-- 6c
      |   |   `-- 68dd37602c8e2036362ab81b12829c4d6c0867
      |   |-- 6d
      |   |   `-- 4b5c23a94a89c7f26266ccf635647fd4002b19
      |   |-- 70
      |   |   |-- 7a20731ff94c2dee063a8b274665b1cc730e26
      |   |   `-- eb05d32223342a549cfb00c20b1464bf1b9513
      |   |-- 71
      |   |   `-- f38e799ec3f84476b6ef128a6bafcadc97a4b1
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
      |   |-- 78
      |   |   `-- 2f6261fa32f8bfec7b89f77bb5cce40c4611cb
      |   |-- 7a
      |   |   `-- c71a2d1648e7de21f4fbe4935cf54b44bfef9a
      |   |-- 7c
      |   |   `-- 30b7adfa79351301a11882adf49f438ec294f8
      |   |-- 7d
      |   |   |-- e033196d3f74f40139647122f499286a97498b
      |   |   `-- fca2962177d9c9925fedf2fbdd79fc7e9309fc
      |   |-- 7f
      |   |   |-- 0f21b330a3d45f363fcde6bfb57eed22948cb6
      |   |   |-- 39c7601f2c1ca44fdc9237efa34f2887daa2b4
      |   |   `-- c8ee5474068055f7740240dfce6fa6e38bbf4d
      |   |-- 82
      |   |   `-- 4c0e846b41e1eb9f95d141b47bbb9ff9baef17
      |   |-- 8c
      |   |   `-- c4bb045e98da7cf00714d91ac77c7ea7e08b63
      |   |-- 93
      |   |   `-- f66d258b7b4c3757e63f985b08f7daa33db64e
      |   |-- 95
      |   |   `-- d99506044285b3088aef86540c478f09606763
      |   |-- 98
      |   |   `-- 84cc2efe368ea0aa9d912fa596b26c5d75dbee
      |   |-- 99
      |   |   `-- acd82fe2a9b89022d1aee5a580c123a8161f4a
      |   |-- 9c
      |   |   `-- 78c532d93505a2a24430635b342b91db22fee0
      |   |-- 9d
      |   |   `-- e3bbb26e2b40f02ca8de195933eb620bbf0b6a
      |   |-- 9e
      |   |   `-- 4d2bcaee240904058a6160e84311667b409b08
      |   |-- 9f
      |   |   |-- 7fe44ebf4b96d3fc03aa7bffff6baa4a84eb63
      |   |   `-- 8daab1754f04fbe8aaac6fcbb44c8324df09eb
      |   |-- a1
      |   |   |-- 269dc50ffcdc1b87664a061926bf9a072a3456
      |   |   `-- c31372c5de4fb705ffdcbf5a4ec5c5103231d9
      |   |-- a4
      |   |   `-- b68220bdf7fb846eb9780f7846a2f4bf7cbcc3
      |   |-- ab
      |   |   `-- a295fbe181a47f04650542b7d5582fbd983b98
      |   |-- b1
      |   |   `-- 55ee8a0221a6d1f94982ab3624f47f7e4931e2
      |   |-- b7
      |   |   `-- bb51e63095336302e4f6b0eead63cb716eb630
      |   |-- b9
      |   |   `-- 90567474f1f8af9784478614483684e88ccf4f
      |   |-- ba
      |   |   `-- f9dcf1394d5152de67b115f55f25e4dc0a2398
      |   |-- bc
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- c1
      |   |   `-- 489fc8fd6ae9ac08c0168d7cabaf5645b922fa
      |   |-- c2
      |   |   `-- d86319b61f31a7f4f1bc89b8ea4356b60c4658
      |   |-- cb
      |   |   `-- bceb2fb07839b8796fadb2b6a8b785b8fd7440
      |   |-- d7
      |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |-- e1
      |   |   |-- 0bf0281a70e6b19939ad6e26e10252bbebe300
      |   |   `-- 25e6d9f8f9acca5ffd25ee3c97d09748ad2a8b
      |   |-- e7
      |   |   `-- cee3592aaac624fd48c258daa5d62d17352043
      |   |-- e8
      |   |   `-- f852fc8816a734b2dd9ffb1a6bb7b92db1af84
      |   |-- e9
      |   |   `-- 9a2c69c0fb10af8dd1524e7f976df3d898f6ac
      |   |-- ea
      |   |   `-- 1ae75547e348b07cb28a721a06ef6580ff67f0
      |   |-- ec
      |   |   `-- 4f59ca1a0ac5b2f375d4917dbba5e6aedff12a
      |   |-- ed
      |   |   `-- efd7dc70381a72b7af5bff2e49b8eb60cb9237
      |   |-- ee
      |   |   `-- b52a9c8fe143baf970160aa4716ff5c019d8cb
      |   |-- f2
      |   |   |-- 257977b96d2272be155d6699046148e477e9fb
      |   |   `-- 7e0d18d976fd84da0a9e260989ecb6edaa593f
      |   |-- f6
      |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |-- f7
      |   |   `-- 35d04266733d64d2f49ab23a183a5207e8961d
      |   |-- fc
      |   |   `-- c182ce4e8039ae321c009746e9a5b42a224bf5
      |   |-- fd
      |   |   `-- 2bc852c86f084dd411054c9c297b05ccf76427
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  147 directories, 147 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
