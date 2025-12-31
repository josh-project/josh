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


  $ mkdir -p sub1/subsub1
  $ echo contents1 > sub1/subsub1/file1
  $ git add .
  $ git commit -m "add subsub1" 1> /dev/null

  $ mkdir -p sub1/subsub2
  $ echo contents1 > sub1/subsub2/file1
  $ git add .
  $ git commit -m "add subsub2" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git clone -q http://localhost:8002/real_repo.git:workspace=ws.git ws
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/ws
  $ cat > workspace.josh <<EOF
  > a/subsub1 = :/sub1/subsub1
  > a/subsub2 = :/sub1/subsub2
  > EOF

  $ git add .
  $ git commit -m "add workspace" 1> /dev/null
  $ git push origin HEAD:refs/heads/master -o merge 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:    81c59c0..37c79e6  JOSH_PUSH -> master
  remote: REWRITE(85ee20960c56619305e098b301d8253888b6ce5b -> 705dcb4e33bd0dd3f95d5831fc8dc8a41ca3e566)
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new branch]      HEAD -> master

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull -q --rebase

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add subsub2
  | * add subsub1
  * add workspace

  $ tree
  .
  |-- a
  |   |-- subsub1
  |   |   `-- file1
  |   `-- subsub2
  |       `-- file1
  `-- workspace.josh
  
  4 directories, 3 files
  $ cat workspace.josh
  a = :/sub1:[
      ::subsub1/
      ::subsub2/
  ]

  $ cd ${TESTTMP}/real_repo
  $ git pull --rebase
  From http://localhost:8001/real_repo
     81c59c0..37c79e6  master     -> origin/master
  Updating 81c59c0..37c79e6
  Fast-forward
   ws/workspace.josh | 4 ++++
   1 file changed, 4 insertions(+)
   create mode 100644 ws/workspace.josh

  $ git log --graph --pretty=%s
  *   Merge from :workspace=ws
  |\  
  | * add workspace
  * add subsub2
  * add subsub1
  * initial

  $ cd ${TESTTMP}/ws
  $ cat > workspace.josh <<EOF
  > a/ = :/sub1
  > EOF

  $ git add workspace.josh
  $ git commit -m "mod workspace" 1> /dev/null

  $ git log --graph --pretty=%s
  * mod workspace
  *   Merge from :workspace=ws
  |\  
  | * add subsub2
  | * add subsub1
  * add workspace


  $ git push 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 500 Internal Server Error
  remote: upstream: response body:
  remote:
  remote:
  remote: Can't apply "mod workspace" (b78eb888451be077531b50794384c2faec025765)
  remote: Invalid workspace:
  remote: ----
  remote:  --> 1:1
  remote:   |
  remote: 1 | a/ = :/sub1
  remote:   | ^---
  remote:   |
  remote:   = expected workspace_file
  remote:
  remote: a/ = :/sub1
  remote:
  remote: ----
  remote: error: hook declined to update refs/heads/master
  To http://localhost:8002/real_repo.git:workspace=ws.git
   ! [remote rejected] master -> master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:workspace=ws.git'


  $ cd ${TESTTMP}/ws
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase
  Current branch master is up to date.

  $ tree
  .
  |-- a
  |   |-- subsub1
  |   |   `-- file1
  |   `-- subsub2
  |       `-- file1
  `-- workspace.josh
  
  4 directories, 3 files

  $ git log --graph --pretty=%s
  * mod workspace
  *   Merge from :workspace=ws
  |\  
  | * add subsub2
  | * add subsub1
  * add workspace

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub1/",
      "::sub1/subsub1/",
      "::sub1/subsub2/",
      "::ws/",
      ":workspace=ws",
  ]
  .
  |-- josh
  |   `-- 25
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
  |   |   |-- 15
  |   |   |   `-- 4448600af55384a18a4f683fea4c3d4cf5290e
  |   |   |-- 1e
  |   |   |   `-- a5cc01771cfa9087f346b7d812dfbe33c1e6b1
  |   |   |-- 26
  |   |   |   `-- cadcac11584c2c798ff38995ebd4d27490885a
  |   |   |-- 2b
  |   |   |   `-- 20b4f8abb6d70648e2573e2f798a18e0079f9e
  |   |   |-- 37
  |   |   |   `-- c79e64948d36bd1bb804e274ef5419bb44e602
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 81
  |   |   |   |-- c59c0666268c2ae7a0d39003e14eb951bb87e3
  |   |   |   `-- cd4ba9bc0f79007940b528627c085f048ec516
  |   |   |-- 88
  |   |   |   `-- e9ef64b0c92e2881fb759e1cf774e75d398d4f
  |   |   |-- 8e
  |   |   |   `-- 4d581b934717eb1f52fcdf21e731e7d7899717
  |   |   |-- 91
  |   |   |   `-- 6a12fa6687633241c43db9a96277d6fd056870
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- b3
  |   |   |   `-- dc01c39cba3251ec3a349fc585bd57ee4136f8
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- eb
  |   |   |   `-- 6a31166c5bf0dbb65c82f89130976a12533ce6
  |   |   |-- f7
  |   |   |   `-- 99a2ffcfae170f01efea806ff109e5e702191a
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
      |   |-- 0a
      |   |   `-- 3e20b963d30a7071105313fada241306df8695
      |   |-- 0f
      |   |   `-- 70f195d1852ceed0a13b3839eee7aeb07571f7
      |   |-- 2b
      |   |   `-- 20b4f8abb6d70648e2573e2f798a18e0079f9e
      |   |-- 31
      |   |   `-- f15ce76ce6a453ecc90f5852e70babf3554707
      |   |-- 37
      |   |   `-- c79e64948d36bd1bb804e274ef5419bb44e602
      |   |-- 3d
      |   |   `-- 96ae1a24134d32cf3eca0629fb4fd1d095693a
      |   |-- 41
      |   |   `-- 3c883bf377b8a2b9ec80ba847a926f5d50e751
      |   |-- 46
      |   |   `-- ac801dc88e2ce2a836fbc7f6e3e35a7c7892ab
      |   |-- 4a
      |   |   `-- 9ee6ea51565adf5c005dd1bc93f4b42f335be3
      |   |-- 4b
      |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
      |   |-- 51
      |   |   `-- 45dedc66248700cf33e354ef555877bc24f533
      |   |-- 66
      |   |   `-- 1bafe5fb60524a9efafd413c42e4c2d706bae8
      |   |-- 6c
      |   |   `-- 8233465e92d353e2ef47c02dc568ea44a32339
      |   |-- 70
      |   |   `-- 5dcb4e33bd0dd3f95d5831fc8dc8a41ca3e566
      |   |-- 7b
      |   |   `-- 418ed7c356797b1a8eef3ff949632495d273c6
      |   |-- 7d
      |   |   `-- 5816334652b9738e33e4ceaf925573c3414e0c
      |   |-- 85
      |   |   `-- ee20960c56619305e098b301d8253888b6ce5b
      |   |-- 88
      |   |   `-- e9ef64b0c92e2881fb759e1cf774e75d398d4f
      |   |-- 8e
      |   |   `-- 4d581b934717eb1f52fcdf21e731e7d7899717
      |   |-- 8f
      |   |   `-- 2bb1e9e57ae77c662a82586d21fb7ee54e7c65
      |   |-- 91
      |   |   `-- 6a12fa6687633241c43db9a96277d6fd056870
      |   |-- 97
      |   |   `-- 398140f110f4009f1fce46e15f9fc140d6908b
      |   |-- 9d
      |   |   `-- 613be55337cdfab189935d8dbd1d4f427ef75e
      |   |-- b7
      |   |   `-- 8eb888451be077531b50794384c2faec025765
      |   |-- cd
      |   |   `-- 9ae8cb61e7d1aaa7b90766ff9aa9b3dc78c856
      |   |-- db
      |   |   `-- 9120ba624b0afe79d37a5a262d8deb14e13707
      |   |-- f0
      |   |   `-- 2803a3f6b703de58a33b330f70b5034e0ebcf8
      |   |-- f7
      |   |   `-- 99a2ffcfae170f01efea806ff109e5e702191a
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  71 directories, 58 files
