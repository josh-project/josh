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
  POST git-receive-pack (1463 bytes)
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
  $ git sync origin HEAD:refs/heads/master -o merge
  * HEAD -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            d91fa4981fe3546f44fa5a779ec6f69b20fdaa0f -> FETCH_HEAD
  HEAD is now at d91fa49 Merge from :workspace=ws
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (445 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    5d605ce..0ebcca7  JOSH_PUSH -> master        
  remote: REWRITE(1b46698f32d1d1db1eaeb34f8c9037778d65f3a9 -> d91fa4981fe3546f44fa5a779ec6f69b20fdaa0f)        
  updating local tracking ref 'refs/remotes/origin/master'
  

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + 1b46698...d91fa49 master     -> origin/master  (forced update)
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

  $ git log --graph --pretty="%s - %an <%ae>"
  *   Merge from :workspace=ws - JOSH <josh@josh-project.dev>
  |\  
  | * add file2 - Josh <josh@example.com>
  | * add file1 - Josh <josh@example.com>
  * add workspace - Josh <josh@example.com>

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     5d605ce..0ebcca7  master     -> origin/master
  Updating 5d605ce..0ebcca7
  Fast-forward
   ws/workspace.josh | 2 ++
   1 file changed, 2 insertions(+)
   create mode 100644 ws/workspace.josh

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add workspace
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
  *   Merge from :workspace=ws
  |\  
  | * add workspace
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
  POST git-receive-pack (795 bytes)
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cd ${TESTTMP}/ws
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
     d91fa49..5d189f7  master     -> origin/master
  Updating d91fa49..5d189f7
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
  *   Merge from :workspace=ws
  |\  
  | * add file2
  | * add file1
  * add workspace

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
  Previous HEAD position was d91fa49 Merge from :workspace=ws
  HEAD is now at 9441c1b add workspace
  $ tree
  .
  `-- workspace.josh
  
  1 directory, 1 file

  $ git checkout master 1> /dev/null
  Previous HEAD position was 9441c1b add workspace
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
   * branch            3fc46d98fe664e12a931c5c1365a1cd845a78a64 -> FETCH_HEAD
  HEAD is now at 3fc46d9 add in filter
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (813 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    f9be76c..c88a8ce  JOSH_PUSH -> master        
  remote: REWRITE(dce83c94807b93f44776c7a1e71cf4f4f8f222b5 -> 3fc46d98fe664e12a931c5c1365a1cd845a78a64)        
  updating local tracking ref 'refs/remotes/origin/master'
  

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > w = :/sub3
  > EOF

  $ git mv d w
  $ git add .
  $ git commit -m "try to modify ws" 1> /dev/null

  $ git sync
    refs/heads/master -> refs/heads/master
  From http://localhost:8002/real_repo.git:workspace=ws
   * branch            14c05920b084cdb89feaa847f9c99d764148ff9b -> FETCH_HEAD
  HEAD is now at 14c0592 try to modify ws
  Pushing to http://localhost:8002/real_repo.git:workspace=ws.git
  POST git-receive-pack (470 bytes)
  remote: josh-proxy: pre-receive hook        
  remote: upstream: response status: 200 OK        
  remote: upstream: response body:        
  remote: 
  remote: To http://localhost:8001/real_repo.git        
  remote:    c88a8ce..381a4ab  JOSH_PUSH -> master        
  remote: REWRITE(7da1ae7f2b93967ad7ea421fa7db95b73b8aa07e -> 14c05920b084cdb89feaa847f9c99d764148ff9b)        
  updating local tracking ref 'refs/remotes/origin/master'
  

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8002/real_repo.git:workspace=ws
   + 7da1ae7...14c0592 master     -> origin/master  (forced update)
  Already up to date.

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
  *   Merge from :workspace=ws
  |\  
  | * add file2
  | * add file1
  * add workspace


  $ cd ${TESTTMP}/real_repo

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  From http://localhost:8001/real_repo
     f9be76c..381a4ab  master     -> origin/master
  Updating f9be76c..381a4ab
  Fast-forward
   sub1/subsub/file1     | 1 -
   sub1/subsub/newfile_1 | 1 +
   sub2/newfile_2        | 1 +
   ws/workspace.josh     | 4 ++--
   ws/ws_file            | 1 +
   5 files changed, 5 insertions(+), 3 deletions(-)
   delete mode 100644 sub1/subsub/file1
   create mode 100644 sub1/subsub/newfile_1
   create mode 100644 sub2/newfile_2
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
      |-- workspace.josh
      `-- ws_file
  
  6 directories, 9 files
  $ git log --graph --pretty=%s
  * try to modify ws
  * add in filter
  * mod workspace
  * add file3
  *   Merge from :workspace=ws
  |\  
  | * add workspace
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
  Previous HEAD position was c88a8ce add in filter
  HEAD is now at f9be76c mod workspace
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
  |       `-- 32
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
  |   |   |-- 04
  |   |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
  |   |   |-- 0d
  |   |   |   `-- 4ddd7b05d80c6b177e125195baba7544999ba1
  |   |   |-- 0e
  |   |   |   `-- bcca72a14d4aa4b50037956fdad1d440deeee1
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
  |   |   |-- 2d
  |   |   |   `-- 1906dd31141f2fbab6485ccd34bbd1ea440464
  |   |   |-- 2f
  |   |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
  |   |   |-- 33
  |   |   |   `-- dcdc06e9d605c8aca2375b96f7d431d2eb41d7
  |   |   |-- 34
  |   |   |   `-- c24765275d6f3ec5d6baeaaa4299471d6f7df0
  |   |   |-- 36
  |   |   |   `-- 52f9baa44258d0f505314830ad37d16eafc981
  |   |   |-- 38
  |   |   |   `-- 1a4abe20b33f38f4b6f559d08a38c59355ff7e
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
  |   |   |-- 59
  |   |   |   `-- 632d8d838ce9390679767c02c6bfe6c0d244a9
  |   |   |-- 5d
  |   |   |   `-- 605cee0c66b1c25a15a2d435b2786cc0bc24c5
  |   |   |-- 60
  |   |   |   `-- cb31dd78d6a5cdee8bfbd165e8c3f674f8e83f
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
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- a1
  |   |   |   `-- 1e8a91058875f157ca1246bdc403b88e93cd94
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- b7
  |   |   |   `-- 85a0b60f6ef7044b4c59c318e18e2c47686085
  |   |   |-- bc
  |   |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
  |   |   |-- c6
  |   |   |   `-- 6fb92e3be8e4dc4c89f94d796f3a4b1833e0fa
  |   |   |-- c8
  |   |   |   `-- 8a8cea02112a17891dfdffe7ebd55efd3a3fa2
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
  |   |   |-- f5
  |   |   |   |-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |   `-- d0c4d5fe3173ba8ca39fc198658487eaab8014
  |   |   |-- f6
  |   |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
  |   |   |-- f9
  |   |   |   `-- be76cb9c282a39cf1384e7cbe3d1fb7d425696
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
      |   |-- 04
      |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
      |   |-- 1b
      |   |   `-- 46698f32d1d1db1eaeb34f8c9037778d65f3a9
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 40
      |   |   `-- c389b6b248e13f3cb88dcd79467d7396a4489e
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
      |   |-- 7d
      |   |   `-- a1ae7f2b93967ad7ea421fa7db95b73b8aa07e
      |   |-- 82
      |   |   `-- 4c0e846b41e1eb9f95d141b47bbb9ff9baef17
      |   |-- 95
      |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
      |   |-- b1
      |   |   `-- 55ee8a0221a6d1f94982ab3624f47f7e4931e2
      |   |-- bc
      |   |   `-- 665856e841c4ae4a956483dc57b2ea4cc20116
      |   |-- d7
      |   |   `-- 330ea337031af43ba1cf6982a873a40b9170ac
      |   |-- dc
      |   |   `-- e83c94807b93f44776c7a1e71cf4f4f8f222b5
      |   |-- f2
      |   |   `-- 257977b96d2272be155d6699046148e477e9fb
      |   |-- f6
      |   |   `-- 3dd93419493d22aeaf6bcb5c0bec4c2701b049
      |   |-- f8
      |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
      |   |-- info
      |   `-- pack
      |       |-- pack-133a4127e8d4c9bdc124e21786a688c4e8f778c9.idx
      |       |-- pack-133a4127e8d4c9bdc124e21786a688c4e8f778c9.pack
      |       |-- pack-59055e54f993a526fd1b0200427456390db5dd2d.idx
      |       |-- pack-59055e54f993a526fd1b0200427456390db5dd2d.pack
      |       |-- pack-721a5e2fcf4ed965e49124b30f161a6faead2313.idx
      |       |-- pack-721a5e2fcf4ed965e49124b30f161a6faead2313.pack
      |       |-- pack-9344ad8c1e2b9aca23229bfb5dfbc1a19f64e2dc.idx
      |       |-- pack-9344ad8c1e2b9aca23229bfb5dfbc1a19f64e2dc.pack
      |       |-- pack-af3e3339c7bd303ace0b6210e7f1c13374606269.idx
      |       |-- pack-af3e3339c7bd303ace0b6210e7f1c13374606269.pack
      |       |-- pack-dc4dca83cf9851930e174c49e92f2de588f25e71.idx
      |       |-- pack-dc4dca83cf9851930e174c49e92f2de588f25e71.pack
      |       |-- pack-e27bba3e1cb78d1a86c190a38d3a40070fef9b53.idx
      |       |-- pack-e27bba3e1cb78d1a86c190a38d3a40070fef9b53.pack
      |       |-- pack-fb51f94a07756db0b90e50e60315402cd82f9ac3.idx
      |       `-- pack-fb51f94a07756db0b90e50e60315402cd82f9ac3.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  87 directories, 89 files

$ cat ${TESTTMP}/josh-proxy.out
