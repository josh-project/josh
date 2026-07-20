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

  $ mkdir sub4
  $ echo contents4 > sub4/file4
  $ git add sub4
  $ git commit -m "add file4" 1> /dev/null
  $ git commit -m "one extra commit" --allow-empty
  [master fb0eb97] one extra commit

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
  * one extra commit
  * add file4
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

  $ git log --graph --pretty=%s
  * add file2
  * add file1
  * one extra commit
  * add workspace

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
  > EOF

  $ git commit -a -F - <<EOF
  > Add new folder
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF
  [master e63efb2] Add new folder
   1 file changed, 1 insertion(+)

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote: REWRITE(e63efb2615e1c17f0d0b6e610da85da09438cd29 -> 9bd58f891b4f17736c1b51903837de717fce13a5)
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new reference]   HEAD -> refs/for/master

  $ cd ${TESTTMP}/remote/real_repo.git/

  $ git update-ref refs/changes/1/1 refs/for/master

  $ git update-ref -d refs/for/master

  $ cd ${TESTTMP}/ws

  $ git fetch -q http://localhost:8002/real_repo.git@refs/changes/1/1:workspace=ws.git && git checkout -q FETCH_HEAD

  $ cat > workspace.josh <<EOF
  > a/b = :/sub2
  > c = :/sub1
  > d = :/sub3
  > e = :/sub4
  > EOF

  $ git commit -aq --amend -F - <<EOF
  > Add new folders
  > 
  > Change-Id: Id6ca199378bf7e543e5e0c20e64d448e4126e695
  > EOF

  $ git push origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 500 Internal Server Error
  remote: upstream: response body:
  remote:
  remote: Rejecting new orphan branch at "Add new folders" (5645805dcc75cfe4922b9cb301c40a4a4b35a59d)
  remote: Specify one of these options:
  remote:   '-o allow_orphans' to keep the history as is
  remote:   '-o merge' to import new history by creating merge commit
  remote:   '-o edit' if you are editing a stored filter or workspace
  remote:
  remote: error: hook declined to update refs/for/master
  To http://localhost:8002/real_repo.git:workspace=ws.git
   ! [remote rejected] HEAD -> refs/for/master (hook declined)
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:workspace=ws.git'
  $ git push -o edit origin HEAD:refs/for/master 2>&1 >/dev/null | sed -e 's/[ ]*$//g'
  remote: josh-proxy: pre-receive hook
  remote: upstream: response status: 200 OK
  remote: upstream: response body:
  remote:
  remote: To http://localhost:8001/real_repo.git
  remote:  * [new reference]   JOSH_PUSH -> refs/for/master
  remote: REWRITE(5645805dcc75cfe4922b9cb301c40a4a4b35a59d -> 9a28fa82a736714d831348bbf62b951be65331b7)
  To http://localhost:8002/real_repo.git:workspace=ws.git
   * [new reference]   HEAD -> refs/for/master


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub1/",
      "::sub1/subsub/",
      "::sub2/",
      "::sub3/",
      "::sub4/",
      "::ws/",
      ":workspace=ws",
  ]
  .
  |-- josh
  |   `-- cache
  |       `-- 31
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
  |   |   |-- 14
  |   |   |   `-- b2fb20fa2ded4b41451bf716e0d4741e4fcf49
  |   |   |-- 16
  |   |   |   `-- f299bec8b6eece08fd28777d7cff5edf6132ed
  |   |   |-- 1c
  |   |   |   `-- b5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  |   |   |-- 22
  |   |   |   `-- f927526ccfaac5b87f90bc1a31ba5bd2d315ab
  |   |   |-- 27
  |   |   |   `-- 5b45aec0a1c944c3a4c71cc71ee08d0c9ea347
  |   |   |-- 28
  |   |   |   `-- 8746e9035732a1fe600ee331de94e70f9639cb
  |   |   |-- 2a
  |   |   |   |-- f771a31e4b62d67b59d74a74aba97d1eadcfab
  |   |   |   `-- f8fd9cc75470c09c6442895133a815806018fc
  |   |   |-- 30
  |   |   |   `-- 48804b01e298df4a6e1bc60a1e3b2ca0b016bd
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 40
  |   |   |   `-- c05934b8b40a6aea7835c4e97f1d2acb06bc97
  |   |   |-- 4d
  |   |   |   `-- aab0f68d3893d3b39725ce9f81d68cc8d5503d
  |   |   |-- 5a
  |   |   |   `-- fcddfe10e63e4b970f0a16ea5ab410bd51c5c7
  |   |   |-- 65
  |   |   |   `-- ca339b2d1d093f69c18e1a752833927c2591e2
  |   |   |-- 82
  |   |   |   `-- 8956f4a5f717b3ba66596cc200e7bb51a5633f
  |   |   |-- 83
  |   |   |   `-- 60d96c8d9e586f0f79d6b712a72d22894840ac
  |   |   |-- 85
  |   |   |   `-- 837e6104d0a81b944c067e16ddc83c7a38739f
  |   |   |-- 88
  |   |   |   `-- 3b1bd99f9c48cec992469c1ec20d2d3ea4bec0
  |   |   |-- 8b
  |   |   |   `-- d303a67f516a2748cedf487129dfb937fcbbf6
  |   |   |-- 90
  |   |   |   `-- 2bb8ff1ff20c4fcc3e2f9dcdf7bfa85e0fc004
  |   |   |-- 95
  |   |   |   `-- 19a72b0b8d581a4e859d412cfe9c2689acac53
  |   |   |-- a0
  |   |   |   |-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |   `-- 9bec5980768ee3584be8ac8f148dd60bac370b
  |   |   |-- a7
  |   |   |   `-- 5eedb18d4cd23e4ad3e5af9c1f71006bc9390b
  |   |   |-- b5
  |   |   |   `-- a6423d90bd82e4473a1bebe68f1295d4f9d6a8
  |   |   |-- c6
  |   |   |   `-- 61ed4784f26f89d47e5ea0be3f404ee494e072
  |   |   |-- d0
  |   |   |   `-- 337df37921f762673a4ee9008f98bf2f9524d3
  |   |   |-- e6
  |   |   |   `-- 9de29bb2d1d6434b8b29ae775ad8c2e48c5391
  |   |   |-- ed
  |   |   |   `-- 42dbbeb77e5cf17175f2a048c97e965507a57d
  |   |   |-- f5
  |   |   |   |-- 386e2d5fba005c1589dcbd9735fa1896af637c
  |   |   |   `-- 719cbf23e85915620cec2b2b8bd6fec8d80088
  |   |   |-- f8
  |   |   |   `-- 5eaa207c7aba64f4deb19a9acd060c254fb239
  |   |   |-- fb
  |   |   |   `-- 0eb97a05a4dabbbf4901729d7189e7d95e732d
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
  |       |               |-- changes
  |       |               |   `-- 1
  |       |               |       `-- 1
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
      |   |-- 02
      |   |   `-- 667f8e29e4b012540e81065f01c16031c2df27
      |   |-- 56
      |   |   `-- 45805dcc75cfe4922b9cb301c40a4a4b35a59d
      |   |-- 58
      |   |   `-- b0c1e483109b33f416e0ae08487b4d1b6bfd5b
      |   |-- 6a
      |   |   `-- 80a5b3af9023d11cb7f37bc1f80d1d1805bfdb
      |   |-- dc
      |   |   `-- 268932c3e0a21d51ec34fb88c6947f51faa430
      |   |-- e6
      |   |   `-- 3efb2615e1c17f0d0b6e610da85da09438cd29
      |   |-- info
      |   `-- pack
      |       |-- pack-020a9c25afd039b7908829ed180c3c5b5c2aeb43.idx
      |       |-- pack-020a9c25afd039b7908829ed180c3c5b5c2aeb43.pack
      |       |-- pack-5e1711c1c8fd3c58e9ecf87c44dda797a57b4d09.idx
      |       |-- pack-5e1711c1c8fd3c58e9ecf87c44dda797a57b4d09.pack
      |       |-- pack-810042fcc342c573b7d491be291eac8697326115.idx
      |       |-- pack-810042fcc342c573b7d491be291eac8697326115.pack
      |       |-- pack-8f2ab97436b3ff8e62f3dce0c1093ce4832bca89.idx
      |       |-- pack-8f2ab97436b3ff8e62f3dce0c1093ce4832bca89.pack
      |       |-- pack-b531b8e915c7145f1ef6b21dba15bd66a73cdc32.idx
      |       |-- pack-b531b8e915c7145f1ef6b21dba15bd66a73cdc32.pack
      |       |-- pack-f3754114f5e623d4b9710175aed17f8bd2ce1f30.idx
      |       `-- pack-f3754114f5e623d4b9710175aed17f8bd2ce1f30.pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  67 directories, 66 files
