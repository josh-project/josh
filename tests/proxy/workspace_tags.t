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
  > c = :/sub1
  > EOF

  $ git add ws
  $ git commit -m "add workspace" 1> /dev/null

  $ echo content1 > file1 1> /dev/null
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

  $ git merge new1 -q --no-ff

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null


  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * add file3
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial
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
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  5 directories, 3 files

  $ cat workspace.josh
  a/b = :/sub2
  c = :/sub1

  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * add workspace

  $ echo newfile_1_contents > c/subsub/newfile_1
  $ echo newfile_2_contents > a/b/newfile_2

  $ git add .

  $ git commit -m "add in filter" 1> /dev/null

  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:    176e8e0..11e2559  JOSH_PUSH -> master
  remote: REWRITE(5fa942ed9d35f280b35df2c4ef7acd23319271a5 -> 2cbcd105ead63a4fecf486b949db7f44710300e5)
  To http://localhost:8002/real_repo.git:workspace=ws.git
     6be0d68..5fa942e  master -> master
  $ git log --graph --oneline
  * 5fa942e add in filter
  * 6be0d68 add file2
  * 833812f add file1
  * 1b46698 add workspace

  $ cd ${TESTTMP}/real_repo
  $ git pull 2>/dev/null 1>/dev/null
  $ git log --graph --oneline
  * 11e2559 add in filter
  * 176e8e0 add file2
  * 76cd9e6 add file1
  * 828956f add file3
  *   65ca339 Merge branch 'new1'
  |\  
  | * 902bb8f add newfile1
  * | f5719cb newfile master
  |/  
  * a75eedb initial
  * 8360d96 add workspace

# Pushing a tag from the workspace on the latest commit. It also gets rewritten, because we didn't
# fetch yet.

  $ cd ${TESTTMP}/ws
  $ git tag tag_from_ws_1

  $ git push origin tag_from_ws_1 -o base=refs/heads/master
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new tag]         JOSH_PUSH -> tag_from_ws_1        
  remote: REWRITE(5fa942ed9d35f280b35df2c4ef7acd23319271a5 -> 2cbcd105ead63a4fecf486b949db7f44710300e5)        
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new tag]         tag_from_ws_1 -> tag_from_ws_1

  $ git fetch --all
  From http://localhost:8002/real_repo.git:workspace=ws
   + 5fa942e...2cbcd10 master     -> origin/master  (forced update)

  $ cd ${TESTTMP}/real_repo

  $ git pull --tags --rebase 1> /dev/null
  From http://localhost:8001/real_repo
   * [new tag]         tag_from_ws_1 -> tag_from_ws_1

  $ git log --tags --graph --pretty="%s %d"
  * add in filter  (HEAD -> master, tag: tag_from_ws_1, origin/master)
  * add file2 
  * add file1 
  * add file3 
  *   Merge branch 'new1' 
  |\  
  | * add newfile1  (new1)
  * | newfile master 
  |/  
  * initial 
  * add workspace 

# Pushing a tag from the workspace on an older commit

  $ cd ${TESTTMP}/ws
  $ git checkout HEAD~3 2>/dev/null
  $ git log --oneline
  1b46698 add workspace
  $ git tag tag_from_ws_2
  $ git push origin tag_from_ws_2 -o base=refs/heads/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new tag]         JOSH_PUSH -> tag_from_ws_2
  remote: warnings:
  remote: No match for "c = :/sub1"
  remote: No match for "a/b = :/sub2"
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new tag]         tag_from_ws_2 -> tag_from_ws_2

  $ cd ${TESTTMP}/real_repo

  $ git pull --tags --rebase 1> /dev/null
  From http://localhost:8001/real_repo
   * [new tag]         tag_from_ws_2 -> tag_from_ws_2

  $ git log --tags --graph --pretty="%s %d"
  * add in filter  (HEAD -> master, tag: tag_from_ws_1, origin/master)
  * add file2 
  * add file1 
  * add file3 
  *   Merge branch 'new1' 
  |\  
  | * add newfile1  (new1)
  * | newfile master 
  |/  
  * initial 
  * add workspace  (tag: tag_from_ws_2)

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
  |   |   |-- 11
  |   |   |   `-- e2559617afa238a8332c15d15fff48d5b57c83
  |   |   |-- 14
  |   |   |   `-- b2fb20fa2ded4b41451bf716e0d4741e4fcf49
  |   |   |-- 17
  |   |   |   `-- 6e8e0eda7dc644342b4cbce4196b968886fff3
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 27
  |   |   |   `-- 5b45aec0a1c944c3a4c71cc71ee08d0c9ea347
  |   |   |-- 2a
  |   |   |   |-- f771a31e4b62d67b59d74a74aba97d1eadcfab
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 2d
  |   |   |   `-- 1906dd31141f2fbab6485ccd34bbd1ea440464
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 5a
  |   |   |   |-- f4045367114a7584eefa64b95bb69d7f840aef
  |   |   |   `-- fcddfe10e63e4b970f0a16ea5ab410bd51c5c7
  |   |   |-- 65
  |   |   |   `-- ca339b2d1d093f69c18e1a752833927c2591e2
  |   |   |-- 76
  |   |   |   `-- cd9e690c1d36eb4cdbf3cd244e9defda4ff3ad
  |   |   |-- 82
  |   |   |   `-- 8956f4a5f717b3ba66596cc200e7bb51a5633f
  |   |   |-- 83
  |   |   |   `-- 60d96c8d9e586f0f79d6b712a72d22894840ac
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 90
  |   |   |   `-- 2bb8ff1ff20c4fcc3e2f9dcdf7bfa85e0fc004
  |   |   |-- 95
  |   |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
  |   |   |-- a0
  |   |   |   |-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |   `-- 9bec5980768ee3584be8ac8f148dd60bac370b
  |   |   |-- a3
  |   |   |   `-- d19dcb2f51fa1efd55250f60df559c2b8270b8
  |   |   |-- a4
  |   |   |   `-- 1772e0c7cdad1a13b7a7bc38c0d382a5a827ce
  |   |   |-- a5
  |   |   |   `-- bc2cb1497c5491656a72647f07791fe11f4d8f
  |   |   |-- a7
  |   |   |   `-- 5eedb18d4cd23e4ad3e5af9c1f71006bc9390b
  |   |   |-- bc
  |   |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
  |   |   |-- c3
  |   |   |   `-- 13e8583c38d3ca1a2d987570f9dde3666eed0c
  |   |   |-- d3
  |   |   |   `-- d2a4d6db7addc2b087dcdb3e63785d3315c00e
  |   |   |-- d7
  |   |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- ed
  |   |   |   `-- 42dbbeb77e5cf17175f2a048c97e965507a57d
  |   |   |-- f5
  |   |   |   |-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |   `-- 719cbf23e85915620cec2b2b8bd6fec8d80088
  |   |   |-- f6
  |   |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
  |   |   |-- f8
  |   |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               |-- heads
  |       |               |   `-- master
  |       |               `-- tags
  |       |                   `-- tag_from_ws_1
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 00
      |   |   `-- e02597bb7117c22dc0b551a4062607895fc3d3
      |   |-- 11
      |   |   `-- e2559617afa238a8332c15d15fff48d5b57c83
      |   |-- 13
      |   |   `-- 7228bdcd2ae40007860a69d05168279d95117a
      |   |-- 19
      |   |   `-- 742d815e52ef8d0bb16ddb1a2b7640c3144d0c
      |   |-- 1b
      |   |   `-- 46698f32d1d1db1eaeb34f8c9037778d65f3a9
      |   |-- 1d
      |   |   `-- 2a35c53f4e5901c9cc083a94b417c15837cad8
      |   |-- 22
      |   |   `-- b3eaf7b374287220ac787fd2bce5958b69115c
      |   |-- 23
      |   |   `-- f7e90462b3fc5db0a26335139e1fe83d04d2cd
      |   |-- 26
      |   |   `-- 6864a895cac573b04a44bd40ee3bd8fe458a5f
      |   |-- 2c
      |   |   `-- bcd105ead63a4fecf486b949db7f44710300e5
      |   |-- 2d
      |   |   `-- 1906dd31141f2fbab6485ccd34bbd1ea440464
      |   |-- 2e
      |   |   `-- aadc29a9215c79ff47c4b3a82a024816eb195a
      |   |-- 39
      |   |   `-- abfc68c47fd430cd9775fc18c9f93bc391052e
      |   |-- 43
      |   |   `-- 52611a9e7c56dfdfeadec043ced6d6ef7a5c33
      |   |-- 45
      |   |   `-- c7960fc4d4b38a6a28dcc27f0ae158afa59747
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 4d
      |   |   `-- 5d64cb11e557bba3e609d2b7a605bb80806e94
      |   |-- 5a
      |   |   `-- f4045367114a7584eefa64b95bb69d7f840aef
      |   |-- 5f
      |   |   `-- a942ed9d35f280b35df2c4ef7acd23319271a5
      |   |-- 60
      |   |   `-- 5066c26f66fca5a424aa32bd042ae71c7c8705
      |   |-- 64
      |   |   `-- d1f8d32b274d8c1eeb69891931f52b6ade9417
      |   |-- 6b
      |   |   `-- e0d68b8e87001c8b91281636e21d6387010332
      |   |-- 78
      |   |   `-- 2f6261fa32f8bfec7b89f77bb5cce40c4611cb
      |   |-- 7f
      |   |   `-- 0f21b330a3d45f363fcde6bfb57eed22948cb6
      |   |-- 83
      |   |   `-- 3812f1557e561166754add564fe32228dd1703
      |   |-- 96
      |   |   `-- f5351b972284f81d7246836f82f6be06c6631f
      |   |-- 98
      |   |   `-- 84cc2efe368ea0aa9d912fa596b26c5d75dbee
      |   |-- 9c
      |   |   `-- f258b407cd9cdba97e16a293582b29d302b796
      |   |-- 9f
      |   |   `-- 8daab1754f04fbe8aaac6fcbb44c8324df09eb
      |   |-- a0
      |   |   `-- 28e4ad33176e7db6f3d4a5fc7e92257cfe213e
      |   |-- a3
      |   |   `-- d19dcb2f51fa1efd55250f60df559c2b8270b8
      |   |-- a4
      |   |   |-- 1772e0c7cdad1a13b7a7bc38c0d382a5a827ce
      |   |   `-- 8223bf4fc7801a0322b4ecaa5ed6a2c5dce7f1
      |   |-- b0
      |   |   `-- fdeb65d9b9069015ef9c0f735a4f6f2f28fe77
      |   |-- b1
      |   |   `-- c1b15558aebbce0682f25933cb729e9acd209c
      |   |-- b6
      |   |   `-- c8440fe2cd36638ddb6b3505c1e8f2202f6191
      |   |-- b8
      |   |   `-- ddfe2d00f876ae2513a5b26a560485762f6bfa
      |   |-- bb
      |   |   `-- bd62ec41c785d12270e69b9d49f9babe62fcd6
      |   |-- bc
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- c2
      |   |   `-- d86319b61f31a7f4f1bc89b8ea4356b60c4658
      |   |-- c5
      |   |   `-- dd0ee2c3106a581cdea7db0c4297ef82c0f874
      |   |-- c6
      |   |   `-- 735a7b0d9da9bf6ef5e445ad2f4ce3d825ceb0
      |   |-- d3
      |   |   `-- d2a4d6db7addc2b087dcdb3e63785d3315c00e
      |   |-- d7
      |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |-- e2
      |   |   `-- 5d071c2db414f24473e3768c063dbcf8c55d04
      |   |-- ea
      |   |   `-- 1ae75547e348b07cb28a721a06ef6580ff67f0
      |   |-- ed
      |   |   `-- 8ae0c02d30bd34d7a8584cb0930d0d7a58df26
      |   |-- f2
      |   |   |-- 257977b96d2272be155d6699046148e477e9fb
      |   |   `-- 7e0d18d976fd84da0a9e260989ecb6edaa593f
      |   |-- f5
      |   |   `-- d0c4d5fe3173ba8ca39fc198658487eaab8014
      |   |-- f6
      |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  107 directories, 99 files

$ cat ${TESTTMP}/josh-proxy.out
