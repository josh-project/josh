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
  
  2 directories, 2 files

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
  
  4 directories, 4 files
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
  
  4 directories, 4 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/sub2',
      ':/ws',
      ':workspace=ws',
  ]
  .
  |-- josh
  |   `-- 12
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
      |   |-- 02
      |   |   `-- b7be05e5c483a13e211c5d3dcc30eb8a7047c9
      |   |-- 0b
      |   |   `-- 976ee9223f2a23c5339d6cb3bda2196dbae6b1
      |   |-- 0f
      |   |   `-- 61d50b4e5a814afb71b3da5a2986efd37bc605
      |   |-- 13
      |   |   `-- 10813ea4e1d46c4c5c59bfdaf97a6de3b24c31
      |   |-- 20
      |   |   `-- c53646e34e6079f7dc3090be5c2c3fdd81a4f3
      |   |-- 28
      |   |   `-- a3e71d163a9eb30c3639b16a16f57077abf29b
      |   |-- 2a
      |   |   `-- 3e798288165d5b090a10460984776489bcc7cc
      |   |-- 2d
      |   |   `-- 1906dd31141f2fbab6485ccd34bbd1ea440464
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 36
      |   |   `-- e70a0bbbb8847771f037d5682f33cd26223bc5
      |   |-- 37
      |   |   `-- c3159b05efb7c51e9157e5140a462898ab1a16
      |   |-- 39
      |   |   `-- abfc68c47fd430cd9775fc18c9f93bc391052e
      |   |-- 3e
      |   |   `-- ade035db71b1ce109c190c3e587aff04b82c55
      |   |-- 43
      |   |   `-- c475ca897b62fd739409aee5db69b0c480aa0d
      |   |-- 45
      |   |   |-- f61831a4d523bb75b6f283e2770f1c86060e15
      |   |   `-- f882bfcdf75d0987e19b48a32717c6d06eabed
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 4c
      |   |   `-- af7229f3533f1e776f493fe4f1bc4cb234e72f
      |   |-- 52
      |   |   `-- 8f8f0a55b99f0324a9f189343c24317e34c60d
      |   |-- 57
      |   |   `-- 034d73887096f7850c3ffa2a61a959c61fa7f1
      |   |-- 59
      |   |   `-- ce9b0ca657f0fd704efc832cea8cf64da5bbf5
      |   |-- 5e
      |   |   `-- a433bcec0b828c04d26f0fb03b40535dc4f855
      |   |-- 64
      |   |   `-- d1f8d32b274d8c1eeb69891931f52b6ade9417
      |   |-- 65
      |   |   `-- b0ecf29d3808ba274af864643f5a3f9c8706f1
      |   |-- 66
      |   |   `-- b81c71c0ad10acdb2b4df3b04eef8abd79e64b
      |   |-- 6e
      |   |   `-- 8d20f7effed65135756c3d3428ccf7d7efb818
      |   |-- 73
      |   |   `-- 3cdae47736c40c40238fa2902185b30a5a1831
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
      |   |-- 77
      |   |   `-- b5962d8bd7ad507a02af9767c4cf68c0781200
      |   |-- 84
      |   |   `-- 2d4785bdebe66a26cd2c094a2373cfc6b936ed
      |   |-- 88
      |   |   `-- 4f260811f923775b9f6433ee9ec063ebe19efd
      |   |-- 95
      |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
      |   |-- 99
      |   |   `-- 8e4d99b52680e8fc6d50d98cddad2a4e36a604
      |   |-- 9d
      |   |   `-- b51080a4d148b32bd4c4e0b39eae8d0b3df763
      |   |-- 9e
      |   |   `-- 4d2bcaee240904058a6160e84311667b409b08
      |   |-- 9f
      |   |   `-- bcd9b793b58ad59d17dfe9f1d18382566bc069
      |   |-- a2
      |   |   `-- ad9d7bed2fe8d70e88273ee480142893bf1a8b
      |   |-- b5
      |   |   `-- f6121773050641aa77d3f4f8f02f1e22d10b2e
      |   |-- ba
      |   |   `-- 300b195019b96ef5d20dfe135143b0e8df7636
      |   |-- bc
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- c1
      |   |   `-- 489fc8fd6ae9ac08c0168d7cabaf5645b922fa
      |   |-- c2
      |   |   `-- d86319b61f31a7f4f1bc89b8ea4356b60c4658
      |   |-- c4
      |   |   `-- c85b2c5c47af364fa064bd2b6523fe98ed3852
      |   |-- d3
      |   |   `-- d2a4d6db7addc2b087dcdb3e63785d3315c00e
      |   |-- d7
      |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |-- ea
      |   |   `-- 1ae75547e348b07cb28a721a06ef6580ff67f0
      |   |-- f2
      |   |   |-- 257977b96d2272be155d6699046148e477e9fb
      |   |   `-- 7e0d18d976fd84da0a9e260989ecb6edaa593f
      |   |-- f5
      |   |   `-- d0c4d5fe3173ba8ca39fc198658487eaab8014
      |   |-- f6
      |   |   |-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |   `-- e8ded2e63ba78ef5e9c02679331383cc4b2203
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  82 directories, 73 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
