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
  * add in filter  (HEAD -> master, tag: tag_from_ws_1, origin/master, origin/HEAD)
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
  * add in filter  (HEAD -> master, tag: tag_from_ws_1, origin/master, origin/HEAD)
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
      |   |-- 5a
      |   |   `-- f4045367114a7584eefa64b95bb69d7f840aef
      |   |-- 5f
      |   |   `-- a942ed9d35f280b35df2c4ef7acd23319271a5
      |   |-- a3
      |   |   `-- d19dcb2f51fa1efd55250f60df559c2b8270b8
      |   |-- bb
      |   |   `-- bd62ec41c785d12270e69b9d49f9babe62fcd6
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
      |       |-- pack-4ee5410a0d07448774a9d777901482c58c903f84.idx
      |       |-- pack-4ee5410a0d07448774a9d777901482c58c903f84.pack
      |       |-- pack-72f0031d0154ceb6432b06e392ae4f19a8cfba65.idx
      |       |-- pack-72f0031d0154ceb6432b06e392ae4f19a8cfba65.pack
      |       |-- pack-969240cacd518199eb056a306b470700114f2177.idx
      |       |-- pack-969240cacd518199eb056a306b470700114f2177.pack
      |       |-- pack-d61f3c8dbb59af6cc0c1604d99c262527ccad92e.idx
      |       `-- pack-d61f3c8dbb59af6cc0c1604d99c262527ccad92e.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  67 directories, 64 files

$ cat ${TESTTMP}/josh-proxy.out
