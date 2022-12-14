  $ . ${TESTDIR}/setup_test_env.sh

  $ git init --bare -q ${TESTTMP}/remote/repo1.git/ 1> /dev/null
  $ git config -f ${TESTTMP}/remote/repo1.git/config http.receivepack true
  $ git init --bare -q ${TESTTMP}/remote/repo2.git/ 1> /dev/null
  $ git config -f ${TESTTMP}/remote/repo2.git/config http.receivepack true

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8001/repo1.git
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/repo1 1> /dev/null
  $ echo content1 > file1 1> /dev/null
  $ git add file1 1> /dev/null
  $ git commit -m "initial1" 1> /dev/null
  $ git push
  To http://localhost:8001/repo1.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8001/repo2.git
  warning: You appear to have cloned an empty repository.
  $ cd ${TESTTMP}/repo2 1> /dev/null
  $ echo content2 > file2 1> /dev/null
  $ git add file2 1> /dev/null
  $ git commit -m "initial2" 1> /dev/null
  $ git push
  To http://localhost:8001/repo2.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo
  $ git commit --allow-empty -m "initial" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git fetch --force http://localhost:8002/repo1.git:prefix=repo1.git master:repo1_in_subdir 1> /dev/null
  warning: no common commits
  From http://localhost:8002/repo1.git:prefix=repo1
   * [new branch]      master     -> repo1_in_subdir
  $ git checkout repo1_in_subdir
  Switched to branch 'repo1_in_subdir'
  $ git log --graph --pretty=%s
  * initial1
  $ tree
  .
  `-- repo1
      `-- file1
  
  1 directory, 1 file

$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git fetch --force http://localhost:8002/repo2.git:prefix=repo2.git master:repo2_in_subdir 1> /dev/null
  From http://localhost:8002/repo2.git:prefix=repo2
   * [new branch]      master     -> repo2_in_subdir
  $ git merge -m "Combine" repo2_in_subdir --allow-unrelated-histories 1> /dev/null

  $ git log --graph --pretty=%s
  *   Combine
  |\  
  | * initial2
  * initial1
  $ tree
  .
  |-- repo1
  |   `-- file1
  `-- repo2
      `-- file2
  
  2 directories, 2 files

  $ git checkout master
  Switched to branch 'master'
  Your branch is up to date with 'origin/master'.

  $ git merge -m "Import 1" repo1_in_subdir --allow-unrelated-histories 1> /dev/null

  $ git log --graph --pretty=%s
  *   Import 1
  |\  
  | *   Combine
  | |\  
  | | * initial2
  | * initial1
  * initial

  $ echo new_content1 > repo1/new_file1 1> /dev/null
  $ git add repo1
  $ git commit -m "add new_file1" 1> /dev/null

  $ tree
  .
  |-- repo1
  |   |-- file1
  |   `-- new_file1
  `-- repo2
      `-- file2
  
  2 directories, 3 files

  $ git push 2> /dev/null

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/repo1.git r1 1> /dev/null
  $ cd r1

  $ git log --graph --pretty=%s
  * add new_file1
  * initial1

  $ tree
  .
  |-- file1
  `-- new_file1
  
  0 directories, 2 files

  $ cd ${TESTTMP}/repo1
  $ echo new_content2 > new_file2 1> /dev/null
  $ git add new_file2 1> /dev/null
  $ git commit -m "add new_file2" 1> /dev/null
  $ git push
  To http://localhost:8001/repo1.git
     e189830..8acb3f4  master -> master

  $ cd ${TESTTMP}/real_repo
  $ git checkout master 1> /dev/null
  Already on 'master'
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git fetch --force http://localhost:8002/repo1.git:prefix=repo1.git master:repo1_in_subdir 2> /dev/null
  $ git log --graph --pretty=%s repo1_in_subdir
  * add new_file2
  * initial1

  $ git merge -m "Import 2" repo1_in_subdir --allow-unrelated-histories 1> /dev/null
  $ tree
  .
  |-- repo1
  |   |-- file1
  |   |-- new_file1
  |   `-- new_file2
  `-- repo2
      `-- file2
  
  2 directories, 4 files

  $ git log --graph --pretty=%s
  *   Import 2
  |\  
  | * add new_file2
  * | add new_file1
  * |   Import 1
  |\ \  
  | * \   Combine
  | |\ \  
  | | |/  
  | |/|   
  | | * initial2
  | * initial1
  * initial

  $ git push 2> /dev/null

  $ cd ${TESTTMP}/r1
  $ curl -s http://localhost:8002/flush
  Flushed credential cache
  $ git pull --rebase 2> /dev/null
  Updating 85c3ce1..6fe45a9
  Fast-forward
   new_file2 | 0
   1 file changed, 0 insertions(+), 0 deletions(-)
   create mode 100644 new_file2
  $ tree
  .
  |-- file1
  |-- new_file1
  `-- new_file2
  
  0 directories, 3 files
  $ git log --graph --pretty=%s
  *   Import 2
  |\  
  | * add new_file2
  * | add new_file1
  |/  
  * initial1

  $ cd ${TESTTMP}/repo1
  $ git commit --amend -m "add great new_file2" 1> /dev/null
  $ git push --force
  To http://localhost:8001/repo1.git
   + 8acb3f4...33b9ecd master -> master (forced update)

  $ cd ${TESTTMP}/real_repo
  $ git checkout master 1> /dev/null
  Already on 'master'
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git fetch --force http://localhost:8002/repo1.git:prefix=repo1.git master:repo1_in_subdir 2> /dev/null
  $ git log --graph --pretty=%s repo1_in_subdir
  * add great new_file2
  * initial1

  $ git merge -m "Import 3" repo1_in_subdir --allow-unrelated-histories 1> /dev/null

  $ git log --graph --pretty=%s
  *   Import 3
  |\  
  | * add great new_file2
  * |   Import 2
  |\ \  
  | * | add new_file2
  | |/  
  * | add new_file1
  * |   Import 1
  |\ \  
  | * \   Combine
  | |\ \  
  | | |/  
  | |/|   
  | | * initial2
  | * initial1
  * initial

  $ git push 2> /dev/null

  $ cd ${TESTTMP}/r1
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git pull --rebase 2> /dev/null
  Updating 6fe45a9..8047211
  Fast-forward
  $ tree
  .
  |-- file1
  |-- new_file1
  `-- new_file2
  
  0 directories, 3 files
  $ git log --graph --pretty=%s
  *   Import 3
  |\  
  | * add great new_file2
  * |   Import 2
  |\ \  
  | * | add new_file2
  | |/  
  * / add new_file1
  |/  
  * initial1


Empty roots should not be dropped -> sha1 equal guarantee for "nop"
  $ cd ${TESTTMP}
$ curl -s http://localhost:8002/flush
Flushed credential cache
  $ git clone -q http://localhost:8002/real_repo.git rr 1> /dev/null
  $ cd rr
  $ git log --graph --pretty=%s
  *   Import 3
  |\  
  | * add great new_file2
  * |   Import 2
  |\ \  
  | * | add new_file2
  | |/  
  * | add new_file1
  * |   Import 1
  |\ \  
  | * \   Combine
  | |\ \  
  | | |/  
  | |/|   
  | | * initial2
  | * initial1
  * initial
  $ tree
  .
  |-- repo1
  |   |-- file1
  |   |-- new_file1
  |   `-- new_file2
  `-- repo2
      `-- file2
  
  2 directories, 4 files

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/repo1',
      '::repo1/',
      '::repo2/',
  ]
  "repo1.git" = [':prefix=repo1']
  "repo2.git" = [':prefix=repo2']
  .
  |-- josh
  |   `-- 14
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
  |   |   |-- 05
  |   |   |   `-- f817563be151d278c6021ef1c8cd643d2b6051
  |   |   |-- 20
  |   |   |   `-- d8b46606bc5a0982127be06396bcc250aa37c2
  |   |   |-- 33
  |   |   |   `-- b9ecdf50077ab1f3e99ba58deedccf7a874e9a
  |   |   |-- 4b
  |   |   |   `-- 825dc642cb6eb9a060e54bf8d69288fbee4904
  |   |   |-- 58
  |   |   |   `-- d391109744bf61f6e0118a15bcb0e720a73edc
  |   |   |-- 5d
  |   |   |   |-- 98d297b3b16d5946dada2496accc9f99dc7056
  |   |   |   `-- ebb339446b2a1070687359250e906e45493c37
  |   |   |-- 5e
  |   |   |   `-- c1a6d7931801b54c885942b687c8eb948c189e
  |   |   |-- 60
  |   |   |   `-- bbf7e0d9cbc75bbc27f6c48a2000259ef0dbb1
  |   |   |-- 66
  |   |   |   `-- 35d16da81e6c791e209053e835e5d1fe4295e6
  |   |   |-- 6e
  |   |   |   `-- 9a62b40f85b0952ea06023a8ee1fb99685e6f7
  |   |   |-- 71
  |   |   |   `-- 28ce7713fc45163e65c7ac4a9057ed1913a569
  |   |   |-- 8a
  |   |   |   `-- cb3f4e4afe9db19fa0ed69097535b733874d36
  |   |   |-- 9c
  |   |   |   `-- b7c3c14c5c084f6a6897b6e6bab231fae98ae5
  |   |   |-- a1
  |   |   |   `-- 21124629c3abdc05b28746b5f5890bd9fb5672
  |   |   |-- ad
  |   |   |   `-- 24149d789e59d4b5f9ce41cda90110ca0f98b7
  |   |   |-- bb
  |   |   |   `-- da8cbc6403022ce120659bb957505fc14e9dc1
  |   |   |-- c4
  |   |   |   |-- 8a5acfa23bd6192d897a1c5ca80a0f1d1b4a62
  |   |   |   `-- df336cf6578b6dac651a33fd265d92308c0e39
  |   |   |-- cd
  |   |   |   `-- 9183f60c957365409843269ecefa3ba30a6dad
  |   |   |-- e1
  |   |   |   `-- 898301e7be2b3450a7b0578bc0dd9abd4f51b1
  |   |   |-- e4
  |   |   |   `-- af7700f8c091d18cc15f39c184490125fb0d17
  |   |   |-- e5
  |   |   |   `-- 2c4e8d6c1f4960b92676424944ff3951f472aa
  |   |   |-- e6
  |   |   |   |-- 7656b1f8854bcc258b9dddddc469d7e6d0b139
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- f9
  |   |   |   `-- 97320406328b16277aca038db747cf60232267
  |   |   |-- fa
  |   |   |   `-- 84d25825561f0e21863ad0e93b58517bcdccfe
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       |-- real_repo.git
  |       |       |   |-- HEAD
  |       |       |   `-- refs
  |       |       |       `-- heads
  |       |       |           `-- master
  |       |       |-- repo1.git
  |       |       |   |-- HEAD
  |       |       |   `-- refs
  |       |       |       `-- heads
  |       |       |           `-- master
  |       |       `-- repo2.git
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
      |   |-- 5d
      |   |   `-- 98d297b3b16d5946dada2496accc9f99dc7056
      |   |-- 6e
      |   |   `-- 9a62b40f85b0952ea06023a8ee1fb99685e6f7
      |   |-- 6f
      |   |   `-- e45a9254eb9dd78951a804fa94a035a937ef0b
      |   |-- 71
      |   |   `-- 28ce7713fc45163e65c7ac4a9057ed1913a569
      |   |-- 80
      |   |   `-- 472110df082d0a921ecdcd6f0dd0021f48e019
      |   |-- 85
      |   |   `-- c3ce1926696ef4bdf2eff358bd83b079dcc8d4
      |   |-- 9c
      |   |   `-- b7c3c14c5c084f6a6897b6e6bab231fae98ae5
      |   |-- c4
      |   |   `-- df336cf6578b6dac651a33fd265d92308c0e39
      |   |-- e5
      |   |   `-- 2c4e8d6c1f4960b92676424944ff3951f472aa
      |   |-- f9
      |   |   `-- 97320406328b16277aca038db747cf60232267
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  66 directories, 54 files
