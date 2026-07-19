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
      |   |-- 04
      |   |   `-- 28323b9901ac5959af01b8686b2524c671adc1
      |   |-- 2f
      |   |   `-- 10c52e8ac3117e818b2b8a527c03d9345104c3
      |   |-- 40
      |   |   `-- c389b6b248e13f3cb88dcd79467d7396a4489e
      |   |-- 75
      |   |   `-- 1ecd943cf17e1530017a1db8006771d6c5c4d4
      |   |-- 7d
      |   |   `-- e033196d3f74f40139647122f499286a97498b
      |   |-- 82
      |   |   `-- 4c0e846b41e1eb9f95d141b47bbb9ff9baef17
      |   |-- 9f
      |   |   `-- 7fe44ebf4b96d3fc03aa7bffff6baa4a84eb63
      |   |-- b1
      |   |   `-- 55ee8a0221a6d1f94982ab3624f47f7e4931e2
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
      |       |-- pack-003dec79becceac35fcf4dc6da69067da9076b43.idx
      |       |-- pack-003dec79becceac35fcf4dc6da69067da9076b43.pack
      |       |-- pack-1ea36f2d466e060c9ea31389ae1b3cd2ef9ee1a0.idx
      |       |-- pack-1ea36f2d466e060c9ea31389ae1b3cd2ef9ee1a0.pack
      |       |-- pack-3bb8600225cc4139a131c5413ee7275d07ccd58a.idx
      |       |-- pack-3bb8600225cc4139a131c5413ee7275d07ccd58a.pack
      |       |-- pack-5f43de7285cc69f6f7f779f1c22bbcc404fea509.idx
      |       |-- pack-5f43de7285cc69f6f7f779f1c22bbcc404fea509.pack
      |       |-- pack-78ce499d43b7df7a0b507140d5134ff90a058eeb.idx
      |       `-- pack-78ce499d43b7df7a0b507140d5134ff90a058eeb.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  82 directories, 78 files

$ cat ${TESTTMP}/josh-proxy.out | grep VIEW
