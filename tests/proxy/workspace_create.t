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


  $ mkdir -p sub1/subsub
  $ echo contents1 > sub1/subsub/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub2
  $ echo contents1 > sub2/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ git sync
  * refs/heads/master -> refs/heads/master
  Pushing to http://localhost:8001/real_repo.git
  POST git-receive-pack (1457 bytes)
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cd ${TESTTMP}
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/ws
  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > EOF

  $ git add .
  $ git commit -m "add workspace" 1> /dev/null
  $ git sync origin HEAD:refs/heads/master -o create
  * HEAD -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            003a2970e4c23b64f915025e9adc2e6ed04bc63a -> FETCH_HEAD
  HEAD is now at 003a297 add workspace
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (440 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    5d605ce..82678b3  JOSH_PUSH -> master        
  remote: REWRITE(1b46698f32d1d1db1eaeb34f8c9037778d65f3a9 -> 003a2970e4c23b64f915025e9adc2e6ed04bc63a)        
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + 1b46698...003a297 master     -> origin/master  (forced update)
  Already up to date.

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
  * add workspace
  * add file2
  * add file1

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     5d605ce..82678b3  master     -> origin/master
  Updating 5d605ce..82678b3
  Fast-forward
   ws/workspace.josh | 2 ++
   1 file changed, 2 insertions(+)
   create mode 100644 ws/workspace.josh

  $ git log --graph --pretty=%s
  * add workspace
  * add file2
  * add file1
  *   Merge branch 'new1'
  |\  
  | * add newfile1
  * | newfile master
  |/  
  * initial

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


  $ git sync
    refs/heads/master -> refs/heads/master
  Pushing to http://localhost:8001/real_repo.git
  POST git-receive-pack (789 bytes)
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cd ${TESTTMP}/ws
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
     003a297..f1f2c1b  master     -> origin/master
  Updating 003a297..f1f2c1b
  Fast-forward
   d/file3        | 1 +
   workspace.josh | 3 ++-
   2 files changed, 3 insertions(+), 1 deletion(-)
   create mode 100644 d/file3

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

  $ git checkout HEAD~1 1> /dev/null
  Previous HEAD position was 003a297 add workspace
  HEAD is now at 2a03ad0 add file2
  $ tree
  .
  |-- a
  |   `-- b
  |       `-- file2
  `-- c
      `-- subsub
          `-- file1
  
  5 directories, 2 files

  $ git checkout master 1> /dev/null
  Previous HEAD position was 2a03ad0 add file2
  Switched to branch 'master'

  $ echo newfile_1_contents > c/subsub/newfile_1
  $ git rm c/subsub/file1
  rm 'c/subsub/file1'
  $ echo newfile_2_contents > a/b/newfile_2
  $ echo ws_file_contents > ws_file

  $ git add .

  $ git commit -m "add in filter" 1> /dev/null

  $ git sync
    refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            2a6aa2a100b34d0d56e4b5f19e9bfdc2cd6f7d54 -> FETCH_HEAD
  HEAD is now at 2a6aa2a add in filter
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (808 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    f021d4b..a1a7760  JOSH_PUSH -> master        
  remote: REWRITE(d681a08f543313f2a8bd86fab920e2271d0403d1 -> 2a6aa2a100b34d0d56e4b5f19e9bfdc2cd6f7d54)        
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > w = :/sub3
  > EOF

  $ cp -r d w
  $ git add .
  $ git commit -m "try to modify ws" 1> /dev/null

  $ git sync
    refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            60bd0e180735e169b5c853545d8b1272ed0fc319 -> FETCH_HEAD
  HEAD is now at 60bd0e1 try to modify ws
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (466 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    a1a7760..108eb9a  JOSH_PUSH -> master        
  remote: REWRITE(b8012aab20a6c6a0c2dc3b428d3578aadc9c527f -> 60bd0e180735e169b5c853545d8b1272ed0fc319)        
  updating local tracking ref 'refs/remotes/origin/master'
  

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + b8012aa...60bd0e1 master     -> origin/master  (forced update)
  Already up to date.

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
  |-- d
  |   `-- file3
  |-- w
  |   `-- file3
  |-- workspace.josh
  `-- ws_file
  
  7 directories, 7 files

  $ cat workspace.josh
  c = :/sub1
  a/b = :/sub2
  w = :/sub3

  $ git log --graph --pretty=%s
  * try to modify ws
  * add in filter
  *   mod workspace
  |\  
  | * add file3
  * add workspace
  * add file2
  * add file1


  $ cd ${TESTTMP}/real_repo

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8001/real_repo
     f021d4b..108eb9a  master     -> origin/master
  Updating f021d4b..108eb9a
  Fast-forward
   sub1/subsub/file1     | 1 -
   sub1/subsub/newfile_1 | 1 +
   sub2/newfile_2        | 1 +
   ws/d/file3            | 1 +
   ws/workspace.josh     | 4 ++--
   ws/ws_file            | 1 +
   6 files changed, 6 insertions(+), 3 deletions(-)
   delete mode 100644 sub1/subsub/file1
   create mode 100644 sub1/subsub/newfile_1
   create mode 100644 sub2/newfile_2
   create mode 100644 ws/d/file3
   create mode 100644 ws/ws_file

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
      |-- d
      |   `-- file3
      |-- workspace.josh
      `-- ws_file
  
  7 directories, 10 files
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

  $ git checkout HEAD~1 1> /dev/null
  Previous HEAD position was a1a7760 add in filter
  HEAD is now at f021d4b mod workspace
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
      "::ws/d/",
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
  |   |   |-- 04
  |   |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
  |   |   |-- 0f
  |   |   |   `-- 7ceed53e5b4ab96efad3c0b77e2c00d10169ba
  |   |   |-- 10
  |   |   |   `-- 8eb9a1d2082ac57860d2358d445156e35558a9
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 1e
  |   |   |   `-- 6ea69c6325d02f1dbc9614935f88ce9d2afbac
  |   |   |-- 2a
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 2c
  |   |   |   `-- 50404f5c69295bd3d4d0cb5475be9cc2aada23
  |   |   |-- 2d
  |   |   |   `-- 1906dd31141f2fbab6485ccd34bbd1ea440464
  |   |   |-- 2f
  |   |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
  |   |   |-- 33
  |   |   |   `-- dcdc06e9d605c8aca2375b96f7d431d2eb41d7
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
  |   |   |-- 5e
  |   |   |   `-- 34ec2fa3c3188874f0a6b12ddf76a167df4229
  |   |   |-- 60
  |   |   |   `-- cb31dd78d6a5cdee8bfbd165e8c3f674f8e83f
  |   |   |-- 6d
  |   |   |   `-- 4b5c23a94a89c7f26266ccf635647fd4002b19
  |   |   |-- 73
  |   |   |   `-- d8490d4c86b1e4bfa023ea950ec67ea4e9ca9a
  |   |   |-- 75
  |   |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
  |   |   |-- 7c
  |   |   |   `-- 5a3be33ee5b7e18364f041b05de8ac08bf82ee
  |   |   |-- 82
  |   |   |   `-- 678b3bcd868634f36ad4ec719cca378028dfa4
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 87
  |   |   |   `-- 7af85c0624835da58fe4b2fa9a259a44213acf
  |   |   |-- 8a
  |   |   |   `-- 7fb63d6ac5e60e16941591969c5f8a8d23b8a5
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a1
  |   |   |   |-- 1e8a91058875f157ca1246bdc403b88e93cd94
  |   |   |   `-- a7760c60ac08ecdcac395823637989a4d681a6
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- b7
  |   |   |   `-- 85a0b60f6ef7044b4c59c318e18e2c47686085
  |   |   |-- bc
  |   |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
  |   |   |-- c6
  |   |   |   `-- 6fb92e3be8e4dc4c89f94d796f3a4b1833e0fa
  |   |   |-- d3
  |   |   |   `-- d2a4d6db7addc2b087dcdb3e63785d3315c00e
  |   |   |-- d7
  |   |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
  |   |   |-- e4
  |   |   |   `-- 5f0325cd9fab82d962b758e556d9bf8079fc37
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- eb
  |   |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
  |   |   |-- f0
  |   |   |   `-- 21d4b73a0df674b5729a23e36a1d5632e63b30
  |   |   |-- f5
  |   |   |   `-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |-- f6
  |   |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
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
      |   |-- 00
      |   |   |-- 3a2970e4c23b64f915025e9adc2e6ed04bc63a
      |   |   `-- b49a990c4dc10c67d8b98761c402436627e716
      |   |-- 02
      |   |   `-- 668d7af968c8eed910db7539a57b18dd62a50e
      |   |-- 04
      |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
      |   |-- 09
      |   |   `-- b613e901acc0b5e4279ec6732261134a06f667
      |   |-- 0c
      |   |   `-- d4309cc22b5903503a7196f49c24cf358a578a
      |   |-- 10
      |   |   `-- 8eb9a1d2082ac57860d2358d445156e35558a9
      |   |-- 13
      |   |   `-- 10813ea4e1d46c4c5c59bfdaf97a6de3b24c31
      |   |-- 14
      |   |   |-- 1a3bdf0a2739ded6e233ababff0cd490fd0c56
      |   |   `-- da1560586adda328cca1fbf58c026d6730444f
      |   |-- 17
      |   |   `-- ac72002199728c133087acfa6b23009e00a52a
      |   |-- 19
      |   |   `-- b9637ac9437ea11d42632cd65ca2313952c32f
      |   |-- 1b
      |   |   `-- 46698f32d1d1db1eaeb34f8c9037778d65f3a9
      |   |-- 20
      |   |   `-- 189c97a0ef53368b7ec335baa7a1b86bd76f8e
      |   |-- 23
      |   |   `-- d20398ffe09015ada5794763b97c750b61bdc4
      |   |-- 27
      |   |   `-- d0eee16bdbe4a1ade0ebf877f97467de3b218e
      |   |-- 2a
      |   |   |-- 03ad0fe1720ee0afc95ba8e1bc38a35b87983f
      |   |   |-- 3e798288165d5b090a10460984776489bcc7cc
      |   |   `-- 6aa2a100b34d0d56e4b5f19e9bfdc2cd6f7d54
      |   |-- 2c
      |   |   |-- 50404f5c69295bd3d4d0cb5475be9cc2aada23
      |   |   `-- 913262d99f8cd15ea711a89e943cd902fb87a0
      |   |-- 2d
      |   |   `-- 1906dd31141f2fbab6485ccd34bbd1ea440464
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 30
      |   |   `-- 48804b01e298df4a6e1bc60a1e3b2ca0b016bd
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
      |   |-- 46
      |   |   `-- bc1eabe4b2029b9fcb661f0d447d8389d17337
      |   |-- 47
      |   |   `-- 8644b35118f1d733b14cafb04c51e5b6579243
      |   |-- 4b
      |   |   |-- 6c1f65548a50c9ee26e3aec66946b48e65d08f
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 4c
      |   |   `-- a9fbf65ee12663ac24db4be4cab10e53d6d6e7
      |   |-- 5b
      |   |   `-- 560252b0c3c6abc5e0327596a49efb15e494cb
      |   |-- 5e
      |   |   |-- 34ec2fa3c3188874f0a6b12ddf76a167df4229
      |   |   `-- 9213824faf1eb95d0c522329518489196374fd
      |   |-- 60
      |   |   `-- bd0e180735e169b5c853545d8b1272ed0fc319
      |   |-- 64
      |   |   |-- 6fd2c5bfe156d57ba03f62f2fe735ddbb74e22
      |   |   `-- d1f8d32b274d8c1eeb69891931f52b6ade9417
      |   |-- 6c
      |   |   `-- 68dd37602c8e2036362ab81b12829c4d6c0867
      |   |-- 6d
      |   |   `-- 4b5c23a94a89c7f26266ccf635647fd4002b19
      |   |-- 70
      |   |   `-- eb05d32223342a549cfb00c20b1464bf1b9513
      |   |-- 71
      |   |   |-- f38e799ec3f84476b6ef128a6bafcadc97a4b1
      |   |   `-- fb4611471b5f67b815e54590ca24224d4f8786
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
      |   |-- 78
      |   |   `-- 2f6261fa32f8bfec7b89f77bb5cce40c4611cb
      |   |-- 7c
      |   |   `-- 30b7adfa79351301a11882adf49f438ec294f8
      |   |-- 7d
      |   |   `-- fca2962177d9c9925fedf2fbdd79fc7e9309fc
      |   |-- 7f
      |   |   |-- 39c7601f2c1ca44fdc9237efa34f2887daa2b4
      |   |   `-- c8ee5474068055f7740240dfce6fa6e38bbf4d
      |   |-- 82
      |   |   |-- 4c0e846b41e1eb9f95d141b47bbb9ff9baef17
      |   |   `-- 678b3bcd868634f36ad4ec719cca378028dfa4
      |   |-- 87
      |   |   `-- 7af85c0624835da58fe4b2fa9a259a44213acf
      |   |-- 8c
      |   |   `-- c4bb045e98da7cf00714d91ac77c7ea7e08b63
      |   |-- 93
      |   |   `-- f66d258b7b4c3757e63f985b08f7daa33db64e
      |   |-- 95
      |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
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
      |   |   `-- 8daab1754f04fbe8aaac6fcbb44c8324df09eb
      |   |-- a1
      |   |   |-- 1e8a91058875f157ca1246bdc403b88e93cd94
      |   |   |-- 269dc50ffcdc1b87664a061926bf9a072a3456
      |   |   |-- a7760c60ac08ecdcac395823637989a4d681a6
      |   |   `-- c31372c5de4fb705ffdcbf5a4ec5c5103231d9
      |   |-- b1
      |   |   `-- c3c9b731fea98c82ce8e9286e213fcb033f70d
      |   |-- b2
      |   |   `-- 7fe8d094037d5b5b911893496bb2ee0f40d820
      |   |-- b7
      |   |   `-- bb51e63095336302e4f6b0eead63cb716eb630
      |   |-- b8
      |   |   `-- 012aab20a6c6a0c2dc3b428d3578aadc9c527f
      |   |-- b9
      |   |   `-- 90567474f1f8af9784478614483684e88ccf4f
      |   |-- ba
      |   |   `-- f9dcf1394d5152de67b115f55f25e4dc0a2398
      |   |-- bc
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- bf
      |   |   `-- 7841c0db704465dda4b387d5da09694647d188
      |   |-- c1
      |   |   `-- 489fc8fd6ae9ac08c0168d7cabaf5645b922fa
      |   |-- c2
      |   |   `-- d86319b61f31a7f4f1bc89b8ea4356b60c4658
      |   |-- cb
      |   |   `-- bceb2fb07839b8796fadb2b6a8b785b8fd7440
      |   |-- d3
      |   |   `-- d2a4d6db7addc2b087dcdb3e63785d3315c00e
      |   |-- d6
      |   |   `-- 81a08f543313f2a8bd86fab920e2271d0403d1
      |   |-- d7
      |   |   |-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |   `-- e36a04a3c59c966815fae6db6fe5d518a3456a
      |   |-- da
      |   |   `-- af0560e4e779353311a9039b31ea4f0f1dec37
      |   |-- e1
      |   |   |-- 0bf0281a70e6b19939ad6e26e10252bbebe300
      |   |   `-- 25e6d9f8f9acca5ffd25ee3c97d09748ad2a8b
      |   |-- e7
      |   |   `-- cee3592aaac624fd48c258daa5d62d17352043
      |   |-- e9
      |   |   `-- 9a2c69c0fb10af8dd1524e7f976df3d898f6ac
      |   |-- ea
      |   |   `-- 1ae75547e348b07cb28a721a06ef6580ff67f0
      |   |-- ec
      |   |   `-- 4f59ca1a0ac5b2f375d4917dbba5e6aedff12a
      |   |-- ee
      |   |   `-- b52a9c8fe143baf970160aa4716ff5c019d8cb
      |   |-- f1
      |   |   `-- f2c1bd855237608d4293f1ee98fee640e78405
      |   |-- f2
      |   |   |-- 257977b96d2272be155d6699046148e477e9fb
      |   |   `-- 7e0d18d976fd84da0a9e260989ecb6edaa593f
      |   |-- f5
      |   |   `-- d0c4d5fe3173ba8ca39fc198658487eaab8014
      |   |-- f6
      |   |   |-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |   `-- 7e3cdbde0a4bc95d09a1344d2d1f163b5aa172
      |   |-- f7
      |   |   `-- 35d04266733d64d2f49ab23a183a5207e8961d
      |   |-- f8
      |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
      |   |-- fd
      |   |   `-- 2bc852c86f084dd411054c9c297b05ccf76427
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  150 directories, 155 files

$ cat ${TESTTMP}/josh-proxy.out
