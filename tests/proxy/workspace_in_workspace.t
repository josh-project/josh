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

  $ git checkout -q HEAD~1 1> /dev/null

  $ tree
  .
  |-- c
  |   `-- subsub
  |       `-- file1
  `-- workspace.josh
  
  3 directories, 2 files

  $ git checkout master 1> /dev/null
  Previous HEAD position was 833812f add file1
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
  remote:    176e8e0..11e2559  JOSH_PUSH -> master
  remote: REWRITE(5fa942ed9d35f280b35df2c4ef7acd23319271a5 -> 2cbcd105ead63a4fecf486b949db7f44710300e5)
  To http://localhost:8002/real_repo.git:workspace=ws.git
     6be0d68..5fa942e  master -> master

  $ cd ${TESTTMP}/real_repo

  $ git pull --rebase 1> /dev/null
  From http://localhost:8001/real_repo
     176e8e0..11e2559  master     -> origin/master

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

  $ cd ..
  $ git clone http://localhost:8002/real_repo.git:workspace=ws2.git ws2
  Cloning into 'ws2'...
  warning: You appear to have cloned an empty repository.
  $ cd ws2
  $ echo "::file1" > workspace.josh
  $ git add workspace.josh
  $ git commit -m "add ws2"
  [master (root-commit) d31a8dc] add ws2
   1 file changed, 1 insertion(+)
   create mode 100644 workspace.josh
  $ git sync -o create
  * refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws2
   * branch            b3be5ad252e0f493a404a8785653065d7e677f21 -> FETCH_HEAD
  HEAD is now at b3be5ad add ws2
  Pushing to http://localhost:8002/real_repo.git:workspace=ws2.git
  POST git-receive-pack (430 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    11e2559..517813c  JOSH_PUSH -> master        
  remote: REWRITE(d31a8dce16b9b197a1411e750602e62d8d2f97ae -> b3be5ad252e0f493a404a8785653065d7e677f21)        
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ echo ":workspace=ws" >> workspace.josh
  $ git commit -a -m "add workspace filter"
  [master e2532f1] add workspace filter
   1 file changed, 1 insertion(+)
  $ git sync
  ! refs/heads/master -> refs/heads/master
  Pushing to http://localhost:8002/real_repo.git:workspace=ws2.git
  POST git-receive-pack (487 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 500 Internal Server Error        
  remote: upstream: response body:        
  remote: 
  remote: 
  remote: Can't apply "add workspace filter" (e2532f1207290ed9a961f9fc377a6b7afe415312)        
  remote: Invalid workspace: not reversible        
  remote: error: hook declined to update refs/heads/master        
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:workspace=ws2.git'
  

  $ git ls-tree -r HEAD
  100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\tfile1 (esc)
  100644 blob 2fa28dd621122cd858cf13f53f88dfe37eef5424\tworkspace.josh (esc)


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
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
  |   `-- cache
  |       `-- 29
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
  |   |   |-- 51
  |   |   |   `-- 7813c12644d29529502e7445a026b549129817
  |   |   |-- 58
  |   |   |   `-- 6af2034d76913e16ad09d5a7b683938badb049
  |   |   |-- 5a
  |   |   |   |-- f4045367114a7584eefa64b95bb69d7f840aef
  |   |   |   `-- fcddfe10e63e4b970f0a16ea5ab410bd51c5c7
  |   |   |-- 65
  |   |   |   `-- ca339b2d1d093f69c18e1a752833927c2591e2
  |   |   |-- 68
  |   |   |   `-- b1430cedd477c8117976bdb8aba3cee8b9aa9e
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
  |   |   |-- b9
  |   |   |   `-- 1faa49e725f148de89346f193a4a4295e071cd
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
      |   |-- 2f
      |   |   `-- a28dd621122cd858cf13f53f88dfe37eef5424
      |   |-- 58
      |   |   `-- 6af2034d76913e16ad09d5a7b683938badb049
      |   |-- 5a
      |   |   `-- f4045367114a7584eefa64b95bb69d7f840aef
      |   |-- 5f
      |   |   `-- a942ed9d35f280b35df2c4ef7acd23319271a5
      |   |-- 68
      |   |   `-- b1430cedd477c8117976bdb8aba3cee8b9aa9e
      |   |-- 9b
      |   |   `-- 518075958ed3bda719b38249cd91fcef1da965
      |   |-- a3
      |   |   `-- d19dcb2f51fa1efd55250f60df559c2b8270b8
      |   |-- bb
      |   |   `-- bd62ec41c785d12270e69b9d49f9babe62fcd6
      |   |-- bc
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- d3
      |   |   `-- 1a8dce16b9b197a1411e750602e62d8d2f97ae
      |   |-- d7
      |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |-- e2
      |   |   `-- 532f1207290ed9a961f9fc377a6b7afe415312
      |   |-- f2
      |   |   `-- 257977b96d2272be155d6699046148e477e9fb
      |   |-- f6
      |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |-- info
      |   `-- pack
      |       |-- pack-61cee1ce849190d88cdfb51f8fd8aea0bcfd8e63.idx
      |       |-- pack-61cee1ce849190d88cdfb51f8fd8aea0bcfd8e63.pack
      |       |-- pack-72f0031d0154ceb6432b06e392ae4f19a8cfba65.idx
      |       |-- pack-72f0031d0154ceb6432b06e392ae4f19a8cfba65.pack
      |       |-- pack-7aa46131845bcb33ada5dd9264a54f6bda553621.idx
      |       |-- pack-7aa46131845bcb33ada5dd9264a54f6bda553621.pack
      |       |-- pack-969240cacd518199eb056a306b470700114f2177.idx
      |       |-- pack-969240cacd518199eb056a306b470700114f2177.pack
      |       |-- pack-b0339b769beb47ea5c28c0a4f4379ab2d18a4b29.idx
      |       `-- pack-b0339b769beb47ea5c28c0a4f4379ab2d18a4b29.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  76 directories, 75 files

