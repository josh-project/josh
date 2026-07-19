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
  > # comment
  > #
  > 
  > # comment 2
  > 
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
  # comment
  #
  
  # comment 2
  
  a/b = :/sub2
  c = :/sub1

  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * add workspace

  $ git checkout -q HEAD~1 1> /dev/null

  $ tree
  .
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  3 directories, 2 files

  $ git checkout master 1> /dev/null
  Previous HEAD position was e27e2ee add file1
  Switched to branch 'master'

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
  remote:    dc5f7e8..bb76696  JOSH_PUSH -> master
  remote: REWRITE(b176252014d4a10d3ec078667ecf45dd9a140951 -> fa3b9622c1bcc8363c27d4eb05d1ae8dae15e871)
  To http://localhost:8002/real_repo.git:workspace=ws.git
     be06ec3..b176252  master -> master

  $ cd ${TESTTMP}/real_repo

  $ git pull --rebase 1> /dev/null
  From http://localhost:8001/real_repo
     dc5f7e8..bb76696  master     -> origin/master

  $ git clean -ffdx 1> /dev/null

  $ tree
  .
  |-- file1
  |-- newfile1
  |-- newfile_master
  |-- sub1
  |   `-- subsub
  |       |-- file1
  |       `-- newfile_1
  |-- sub2
  |   |-- file2
  |   `-- newfile_2
  |-- sub3
  |   `-- file3
  `-- ws
      `-- workspace.josh
  
  6 directories, 9 files

  $ cat ws/workspace.josh
  # comment
  #
  
  # comment 2
  
  c = :/sub1
  a/b = :/sub2

  $ git log --graph --pretty=%s
  * add in filter
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
  |   `-- cache
  |       `-- 30
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
  |   |   |-- 11
  |   |   |   `-- 731d321eddb29674191126ed5ce3413778ed14
  |   |   |-- 15
  |   |   |   `-- 2ebf6a60e4428105c586b12ee7aeb6f93b5653
  |   |   |-- 19
  |   |   |   `-- 279da7740f0db978e164b200fe34169f3c633c
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 2a
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 4b
  |   |   |   `-- 7fa9ec1191b73a254422912b7cc2ce0abb78dc
  |   |   |-- 56
  |   |   |   `-- 11a41651a4fa991359cdf42033d6c898e6de31
  |   |   |-- 67
  |   |   |   `-- b73963ec5931b9643bf807162edf17636c1f20
  |   |   |-- 74
  |   |   |   `-- 0fc371f1c763aa861ac545dbb1c776ee44eb61
  |   |   |-- 76
  |   |   |   `-- 3691155f96d914089c1907339635f396254786
  |   |   |-- 7f
  |   |   |   `-- b42a80a5502f047ac602ba190f477c06b9e2df
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 90
  |   |   |   `-- ef73747f450299b3dd21586c3e0253e901eaac
  |   |   |-- 93
  |   |   |   `-- 24e89dea23615a773d6c11dfc1449ee46ff49e
  |   |   |-- 9c
  |   |   |   `-- 99a1f2c6ff42ff8e15218590173a242edbe6b6
  |   |   |-- 9f
  |   |   |   `-- e18c87ee4cc1d96e0b62880ac6be1c42b30d4b
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- d0
  |   |   |   `-- 631b4684c4ba6c6f3a533780ac946649d893b7
  |   |   |-- d5
  |   |   |   `-- 18148177d23b6fd4ea7834fe9c0a462661576f
  |   |   |-- dc
  |   |   |   `-- 5f7e833b5a58dfd6fb216ff1867c14bf9c61cb
  |   |   |-- e5
  |   |   |   `-- 98bc9f9a8557dd6411fe3f2b1d82c387ede41e
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- f5
  |   |   |   `-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |-- fa
  |   |   |   `-- 42650f00240a3fcde5aa7e4850f925a97a48d0
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
      |   |-- 5a
      |   |   `-- f4045367114a7584eefa64b95bb69d7f840aef
      |   |-- a3
      |   |   `-- d19dcb2f51fa1efd55250f60df559c2b8270b8
      |   |-- a4
      |   |   `-- 36b5a3ef821ad5db735ff557d1cb2c8cbb3599
      |   |-- b1
      |   |   `-- 76252014d4a10d3ec078667ecf45dd9a140951
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
      |       |-- pack-5d11ca698a761cfb0102d4d44599349077fbafb5.idx
      |       |-- pack-5d11ca698a761cfb0102d4d44599349077fbafb5.pack
      |       |-- pack-cf634696f53f4ca2e6072988b224113252440145.idx
      |       |-- pack-cf634696f53f4ca2e6072988b224113252440145.pack
      |       |-- pack-e5529b295aa82690ce40a2abeefb9ae9bdfe2288.idx
      |       `-- pack-e5529b295aa82690ce40a2abeefb9ae9bdfe2288.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  61 directories, 52 files

$ cat ${TESTTMP}/josh-proxy.out
