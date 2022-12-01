Setup

  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

Clone an empty repo

  $ git clone -q http://localhost:8001/real_repo.git >/dev/null 2>&1
  $ cd real_repo

Commit a file in a root folder

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "add file1"
  [master (root-commit) 0b4cf6c] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 file1

Commit a file in a subfolder and push

  $ mkdir sub
  $ echo contents2 > sub/file2
  $ git add sub
  $ git commit -m "add file2" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

Check commit SHA1
  $ SHA1=$(git log --max-count=1 --format="%H")
  $ echo "${SHA1}"
  37c3f9a18f21fe53e0be9ea657220ba4537dbca7

Clone subfolder as a workspace

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub.git
  $ cd sub

Check workspace contents

  $ ls
  file2

Create a new branch and push it

  $ git switch -c new-branch
  Switched to a new branch 'new-branch'
  $ git push origin new-branch -o base=refs/heads/master 1> /dev/null
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:  * [new branch]      JOSH_PUSH -> new-branch        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub.git
   * [new branch]      new-branch -> new-branch
Check the branch pushed
  $ cd ${TESTTMP}/real_repo
  $ git fetch
  From http://localhost:8001/real_repo
   * [new branch]      new-branch -> origin/new-branch
  $ [ "${SHA1}" = "$(git log --max-count=1 --format='%H' origin/new-branch)" ] || echo "SHA1 differs after push!"

Add one more commit in the workspace and push using implicit prefix in base

  $ cd ${TESTTMP}/sub
  $ echo test > test.txt
  $ git add test.txt
  $ git commit -m "test commit"
  [new-branch 751ef45] test commit
   1 file changed, 1 insertion(+)
   create mode 100644 test.txt
  $ git push origin new-branch -o base=master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    37c3f9a..56dc1f7  JOSH_PUSH -> new-branch        
  remote: 
  remote: 
  To http://localhost:8002/real_repo.git:/sub.git
     28d2085..751ef45  new-branch -> new-branch

Check the branch again

  $ cd ${TESTTMP}/real_repo
  $ git fetch
  From http://localhost:8001/real_repo
     37c3f9a..56dc1f7  new-branch -> origin/new-branch
  $ [ "${SHA1}" = "$(git log --max-count=1 --skip=1 --format='%H' origin/new-branch)" ] || echo "SHA1 differs after push!"

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub']
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
  |   |   |-- 0b
  |   |   |   `-- 4cf6c9efbbda1eada39fa9c1d21d2525b027bb
  |   |   |-- 37
  |   |   |   `-- c3f9a18f21fe53e0be9ea657220ba4537dbca7
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- 5f
  |   |   |   `-- 2752aa0d3b643a6e95d754c3fd272318a02434
  |   |   |-- 6b
  |   |   |   `-- 46faacade805991bcaea19382c9d941828ce80
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- ae
  |   |   |   `-- a557394ce29f000108607abd97f19fed4d1b7c
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
  |       |                   |-- master
  |       |                   `-- new-branch
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 28
      |   |   `-- d20855c7b65b5a9948283516ae62739360544d
      |   |-- 49
      |   |   `-- b12216dab2cefdb1cc0fcda7ab6bc9f8b882ab
      |   |-- 56
      |   |   `-- dc1f749ea31f735f981a42bc6c23e92baf2085
      |   |-- 75
      |   |   `-- 1ef4576e133fc6279ccf882cb812a9b4dcf5dd
      |   |-- 9d
      |   |   `-- aeafb9864cf43055ae93beb0afd6c7d144bfa4
      |   |-- a5
      |   |   `-- 5a119d24890de3a3e470f941217479629e50c6
      |   |-- b5
      |   |   `-- afbb444fd22857e78ee11ddd92b7dd2f5c7d11
      |   |-- de
      |   |   `-- 7cba2eb70af5ce3555c3670e7641f2f547db74
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  41 directories, 29 files
