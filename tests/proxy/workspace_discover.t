  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real/repo2.git
  warning: You appear to have cloned an empty repository.


  $ cd repo2

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir ws2
  $ cat > ws2/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add ws
  $ git add ws2
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

  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ git push
  To http://localhost:8001/real/repo2.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real/repo2.git:workspace=ws.git ws

  $ sleep 10

  $ curl -s http://localhost:8002/filters
  "real/repo2.git" = [
      "::sub1/",
      "::sub1/subsub/",
      "::sub2/",
      "::sub3/",
      "::ws/",
      "::ws2/",
      ":workspace=ws",
      ":workspace=ws2",
  ]

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real/repo2.git" = [
      "::sub1/",
      "::sub1/subsub/",
      "::sub2/",
      "::sub3/",
      "::ws/",
      "::ws2/",
      ":workspace=ws",
      ":workspace=ws2",
  ]
  .
  |-- josh
  |   `-- 17
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
  |   |   |-- 00
  |   |   |   `-- 2be85829685bf007607e29f41876f8545b49b4
  |   |   |-- 07
  |   |   |   `-- 5d66afd812cff42ae5cb2c519c9ec4633a27b6
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 2a
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 31
  |   |   |   `-- 29b2527c5f75549a92894a1880721fa41f71cb
  |   |   |-- 36
  |   |   |   `-- a3ec5b54f46139a654590771ef2105c8bc3e39
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 46
  |   |   |   `-- 3a4148064e9b240f94d89c2ac1b364a3b3e0fb
  |   |   |-- 54
  |   |   |   `-- 0973df4b43b3a62a587478586b5e30a43f641f
  |   |   |-- 64
  |   |   |   `-- e9e79e2a7ea34aee2245d66978eb35061f937c
  |   |   |-- 6e
  |   |   |   `-- 42c071a7fdd44d6553ef80961792d02eadb426
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 95
  |   |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
  |   |   |-- 9a
  |   |   |   `-- 23bc8b294f42dc6dbc5819b740080136a01747
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a1
  |   |   |   `-- 5b03f87416c0047cadcd4df249b724457e2474
  |   |   |-- c7
  |   |   |   `-- 98928982c619b7a84367d1839641e65b5cff95
  |   |   |-- da
  |   |   |   `-- 8f70fcf336cf187d9087e5cb1fdb2f02d030a7
  |   |   |-- db
  |   |   |   `-- 776e24b339cb0942b25e93e0015f50d8a5db5d
  |   |   |-- dc
  |   |   |   `-- 43836f860aafe77011c0c258d8719c6a125fe8
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- ec
  |   |   |   `-- 5fc80d09f99c7420410c357d5377dc712155f3
  |   |   |-- f5
  |   |   |   `-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |-- f8
  |   |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
  |   |   |-- ff
  |   |   |   `-- 86946d0cbeb90f4c84f8824c5fd617299ee895
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real%2Frepo2.git
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
      |   |-- 1b
      |   |   `-- 46698f32d1d1db1eaeb34f8c9037778d65f3a9
      |   |-- 22
      |   |   `-- b3eaf7b374287220ac787fd2bce5958b69115c
      |   |-- 27
      |   |   `-- 5b45aec0a1c944c3a4c71cc71ee08d0c9ea347
      |   |-- 39
      |   |   `-- abfc68c47fd430cd9775fc18c9f93bc391052e
      |   |-- 43
      |   |   `-- 52611a9e7c56dfdfeadec043ced6d6ef7a5c33
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 6b
      |   |   `-- e0d68b8e87001c8b91281636e21d6387010332
      |   |-- 78
      |   |   `-- 2f6261fa32f8bfec7b89f77bb5cce40c4611cb
      |   |-- 7f
      |   |   `-- 0f21b330a3d45f363fcde6bfb57eed22948cb6
      |   |-- 83
      |   |   `-- 3812f1557e561166754add564fe32228dd1703
      |   |-- 98
      |   |   `-- 84cc2efe368ea0aa9d912fa596b26c5d75dbee
      |   |-- 9c
      |   |   `-- f258b407cd9cdba97e16a293582b29d302b796
      |   |-- 9f
      |   |   `-- 8daab1754f04fbe8aaac6fcbb44c8324df09eb
      |   |-- b6
      |   |   `-- c8440fe2cd36638ddb6b3505c1e8f2202f6191
      |   |-- c2
      |   |   `-- d86319b61f31a7f4f1bc89b8ea4356b60c4658
      |   |-- f2
      |   |   `-- 7e0d18d976fd84da0a9e260989ecb6edaa593f
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  68 directories, 54 files

$ cat ${TESTTMP}/josh-proxy.out
