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

  $ git merge -q new1 --no-ff

  $ mkdir sub3
  $ echo contents3 > sub3/file3
  $ git add sub3
  $ git commit -m "add file3" 1> /dev/null

  $ mkdir sub4
  $ echo contents4 > sub4/file4
  $ git add sub4
  $ git commit -m "add file4" 1> /dev/null
  $ git commit -m "one extra commit" --allow-empty
  [master fb0eb97] one extra commit

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
  * one extra commit
  * add file4
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

  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * one extra commit
  * add workspace

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
  > EOF

  $ git commit -a -F - <<EOF
  > Add new folder
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF
  [master e63efb2] Add new folder
   1 file changed, 1 insertion(+)

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote: REWRITE(e63efb2615e1c17f0d0b6e610da85da09438cd29 -> 9bd58f891b4f17736c1b51903837de717fce13a5)
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new reference]   HEAD -> refs/for/master

  $ cd ${TESTTMP}/remote/real_repo.git/

  $ git update-ref refs/changes/1/1 refs/for/master

  $ git update-ref -d refs/for/master

  $ cd ${TESTTMP}/ws

  $ git fetch -q http://localhost:8002/real_repo.git@refs/changes/1/1:workspace=ws.git && git checkout -q FETCH_HEAD

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
  > e = :/sub4
  > EOF

  $ git commit -aq --amend -F - <<EOF
  > Add new folders
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote: REWRITE(5645805dcc75cfe4922b9cb301c40a4a4b35a59d -> 9a28fa82a736714d831348bbf62b951be65331b7)
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new reference]   HEAD -> refs/for/master


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub1/",
      "::sub1/subsub/",
      "::sub2/",
      "::sub3/",
      "::sub4/",
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
  |   |   |-- 14
  |   |   |   `-- b2fb20fa2ded4b41451bf716e0d4741e4fcf49
  |   |   |-- 16
  |   |   |   `-- f299bec8b6eece08fd28777d7cff5edf6132ed
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 22
  |   |   |   `-- f927526ccfaac5b87f90bc1a31ba5bd2d315ab
  |   |   |-- 27
  |   |   |   `-- 5b45aec0a1c944c3a4c71cc71ee08d0c9ea347
  |   |   |-- 28
  |   |   |   `-- 8746e9035732a1fe600ee331de94e70f9639cb
  |   |   |-- 2a
  |   |   |   |-- f771a31e4b62d67b59d74a74aba97d1eadcfab
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 30
  |   |   |   `-- 48804b01e298df4a6e1bc60a1e3b2ca0b016bd
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 40
  |   |   |   `-- c05934b8b40a6aea7835c4e97f1d2acb06bc97
  |   |   |-- 4d
  |   |   |   `-- aab0f68d3893d3b39725ce9f81d68cc8d5503d
  |   |   |-- 5a
  |   |   |   `-- fcddfe10e63e4b970f0a16ea5ab410bd51c5c7
  |   |   |-- 65
  |   |   |   `-- ca339b2d1d093f69c18e1a752833927c2591e2
  |   |   |-- 82
  |   |   |   `-- 8956f4a5f717b3ba66596cc200e7bb51a5633f
  |   |   |-- 83
  |   |   |   `-- 60d96c8d9e586f0f79d6b712a72d22894840ac
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 88
  |   |   |   `-- 3b1bd99f9c48cec992469c1ec20d2d3ea4bec0
  |   |   |-- 8b
  |   |   |   `-- d303a67f516a2748cedf487129dfb937fcbbf6
  |   |   |-- 90
  |   |   |   `-- 2bb8ff1ff20c4fcc3e2f9dcdf7bfa85e0fc004
  |   |   |-- 95
  |   |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
  |   |   |-- a0
  |   |   |   |-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |   `-- 9bec5980768ee3584be8ac8f148dd60bac370b
  |   |   |-- a7
  |   |   |   `-- 5eedb18d4cd23e4ad3e5af9c1f71006bc9390b
  |   |   |-- b5
  |   |   |   `-- a6423d90bd82e4473a1bebe68f1295d4f9d6a8
  |   |   |-- c6
  |   |   |   `-- 61ed4784f26f89d47e5ea0be3f404ee494e072
  |   |   |-- d0
  |   |   |   `-- 337df37921f762673a4ee9008f98bf2f9524d3
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- ed
  |   |   |   `-- 42dbbeb77e5cf17175f2a048c97e965507a57d
  |   |   |-- f5
  |   |   |   |-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |   `-- 719cbf23e85915620cec2b2b8bd6fec8d80088
  |   |   |-- f8
  |   |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
  |   |   |-- fb
  |   |   |   `-- 0eb97a05a4dabbbf4901729d7189e7d95e732d
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
  |       |               |-- changes
  |       |               |   `-- 1
  |       |               |       `-- 1
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
      |   |   `-- 0c330fa2c083613dfd2b0dce1dde1201b6357c
      |   |-- 02
      |   |   `-- 667f8e29e4b012540e81065f01c16031c2df27
      |   |-- 03
      |   |   `-- abc9b48b4da3daca498937b225ff8d54ba8c56
      |   |-- 0c
      |   |   `-- d4309cc22b5903503a7196f49c24cf358a578a
      |   |-- 16
      |   |   `-- f299bec8b6eece08fd28777d7cff5edf6132ed
      |   |-- 19
      |   |   `-- cff5ef0fce401a1a33c2ac2713d6356cbc1b15
      |   |-- 1b
      |   |   `-- 46698f32d1d1db1eaeb34f8c9037778d65f3a9
      |   |-- 1f
      |   |   `-- 8abd6f6494fc2f715b3c1a39ac2e01ba806938
      |   |-- 20
      |   |   `-- 31777de79cd7c74834915674377e96d6864cc9
      |   |-- 22
      |   |   `-- b3eaf7b374287220ac787fd2bce5958b69115c
      |   |-- 26
      |   |   |-- 2c83b56549e1690e3f878c4df4be1af11c19f0
      |   |   `-- 6864a895cac573b04a44bd40ee3bd8fe458a5f
      |   |-- 28
      |   |   `-- 619ba172ac02f6f6ee83091721f9b345648ec9
      |   |-- 2e
      |   |   `-- 994f29aa1828fece591a9d63a61e93b4c2c629
      |   |-- 30
      |   |   `-- 48804b01e298df4a6e1bc60a1e3b2ca0b016bd
      |   |-- 39
      |   |   `-- abfc68c47fd430cd9775fc18c9f93bc391052e
      |   |-- 3e
      |   |   `-- c3e854c68442bfaa047033e1ade729892017a0
      |   |-- 42
      |   |   `-- 71a51984620f9bb8706bcbfb80d33f66d99dfc
      |   |-- 43
      |   |   `-- 52611a9e7c56dfdfeadec043ced6d6ef7a5c33
      |   |-- 47
      |   |   `-- 8644b35118f1d733b14cafb04c51e5b6579243
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 4d
      |   |   `-- aab0f68d3893d3b39725ce9f81d68cc8d5503d
      |   |-- 50
      |   |   |-- 207be2e0fadfbe2ca8d5e0616a71e7ec01f3e2
      |   |   `-- 724c0a6bac8e87c89b64f6c409a2e0382ff65e
      |   |-- 55
      |   |   `-- 652697c232470cde4141b0e1bbbe2c6ac91260
      |   |-- 56
      |   |   `-- 45805dcc75cfe4922b9cb301c40a4a4b35a59d
      |   |-- 57
      |   |   `-- a36663dff20a0906952548a999b9d3ff770dc4
      |   |-- 58
      |   |   `-- b0c1e483109b33f416e0ae08487b4d1b6bfd5b
      |   |-- 5e
      |   |   `-- 7ff045529989036cbd11bc32b2eaf5a8311aea
      |   |-- 60
      |   |   `-- 5066c26f66fca5a424aa32bd042ae71c7c8705
      |   |-- 6a
      |   |   `-- 80a5b3af9023d11cb7f37bc1f80d1d1805bfdb
      |   |-- 6c
      |   |   `-- 68dd37602c8e2036362ab81b12829c4d6c0867
      |   |-- 74
      |   |   `-- 3fcd7100630aea3ab423c23ec9c012549467ad
      |   |-- 75
      |   |   `-- e89ed8367a6ac09038ca4517967569e1c60858
      |   |-- 76
      |   |   `-- 9e718288ea6c1390adb2b1b6cd8c2c73f2785b
      |   |-- 78
      |   |   `-- 2f6261fa32f8bfec7b89f77bb5cce40c4611cb
      |   |-- 7c
      |   |   `-- 30b7adfa79351301a11882adf49f438ec294f8
      |   |-- 7f
      |   |   `-- 0f21b330a3d45f363fcde6bfb57eed22948cb6
      |   |-- 89
      |   |   `-- ae198bc1b2f11718bd1e76fbe6473054801274
      |   |-- 8f
      |   |   `-- 1b78740f35dafecc063980e2afb231b9ee74a3
      |   |-- 91
      |   |   `-- c0b3ea5e7c1dbeae440c93116450f6c4de65c1
      |   |-- 93
      |   |   `-- f66d258b7b4c3757e63f985b08f7daa33db64e
      |   |-- 97
      |   |   `-- 738bf1d1a305512158d536564d3fccbcb0dbec
      |   |-- 98
      |   |   `-- 84cc2efe368ea0aa9d912fa596b26c5d75dbee
      |   |-- 9a
      |   |   `-- 28fa82a736714d831348bbf62b951be65331b7
      |   |-- 9b
      |   |   `-- d58f891b4f17736c1b51903837de717fce13a5
      |   |-- 9c
      |   |   `-- f258b407cd9cdba97e16a293582b29d302b796
      |   |-- 9f
      |   |   |-- 24b55d4263082d93987e2c0ff6b24df3323f5b
      |   |   `-- 8daab1754f04fbe8aaac6fcbb44c8324df09eb
      |   |-- a7
      |   |   |-- 7106b607ba6489028e85eeec937463cc29c39a
      |   |   `-- cf4e83688bf0ec633d4e4abae4b74dce4852ba
      |   |-- af
      |   |   `-- 7c13846465562922d156aef649f6844d51798b
      |   |-- b5
      |   |   `-- c12ea9494f5e3824d5f7e979dcc56d898036b6
      |   |-- b6
      |   |   |-- c8440fe2cd36638ddb6b3505c1e8f2202f6191
      |   |   `-- cfe79e25ecd337b379e7ec06d7939dbcb2f6a0
      |   |-- bd
      |   |   `-- 56f16bf42ff74e68cfb7a59869c81920b02b87
      |   |-- be
      |   |   `-- c9383652a22b8a07acb86d5357a75f988286dc
      |   |-- bf
      |   |   `-- a4b41bb449aa6f5f0be272340b83b3f3317ff8
      |   |-- c2
      |   |   `-- d86319b61f31a7f4f1bc89b8ea4356b60c4658
      |   |-- c5
      |   |   `-- ab31f80e2b8c97a7d354d252272a9eaebd4581
      |   |-- c7
      |   |   `-- c20449d3cd7e2084419fa525c7b36eb7d5ef8c
      |   |-- d3
      |   |   `-- d28f0a10d8f6be1a5f85c80e3c40bb2b5671cb
      |   |-- d4
      |   |   `-- c6c9ce1c5286d73c55da95d50fbf65ed90bcea
      |   |-- d8
      |   |   `-- 631b65275580884aa3cfbac4b2aafc570fb616
      |   |-- d9
      |   |   `-- 9cfb874f6f7317db8ce0224aa80dd2ba462570
      |   |-- dc
      |   |   `-- 268932c3e0a21d51ec34fb88c6947f51faa430
      |   |-- dd
      |   |   |-- 29249d0f31950d5337ec484230651c3c4cf8ad
      |   |   `-- 9ebd9f693084e229dbcc0998906e42eab1acd5
      |   |-- e1
      |   |   `-- 25e6d9f8f9acca5ffd25ee3c97d09748ad2a8b
      |   |-- e5
      |   |   `-- a8caaa59058b8beb8a603a3b4447c07218a617
      |   |-- e6
      |   |   `-- 3efb2615e1c17f0d0b6e610da85da09438cd29
      |   |-- e9
      |   |   `-- 9a2c69c0fb10af8dd1524e7f976df3d898f6ac
      |   |-- ec
      |   |   `-- 4f59ca1a0ac5b2f375d4917dbba5e6aedff12a
      |   |-- f2
      |   |   `-- 7e0d18d976fd84da0a9e260989ecb6edaa593f
      |   |-- fd
      |   |   `-- 2bc852c86f084dd411054c9c297b05ccf76427
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  129 directories, 123 files
