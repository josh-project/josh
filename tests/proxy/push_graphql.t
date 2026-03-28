  $ . ${TESTDIR}/setup_test_env.sh

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8001/real_repo.git 1> /dev/null
  warning: You appear to have cloned an empty repository.
  $ cd real_repo
  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ git log --graph --pretty=%H
  * bb282e9cdc1b972fffd08fd21eead43bc0c83cb8

  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   rev(at: \"bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\") {
  >     push(target:\"refs/heads/newbranch\")
  >   }
  > }"}
  > EOF

  $ git ls-remote --symref
  From http://localhost:8001/real_repo.git
  ref: refs/heads/master\tHEAD (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\tHEAD (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\trefs/heads/master (esc)
  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "push": true
      }
    }
  }
  $ git ls-remote --symref http://localhost:8001/real_repo.git
  ref: refs/heads/master\tHEAD (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\tHEAD (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\trefs/heads/master (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\trefs/heads/newbranch (esc)

  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   rev(at: \"bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\", filter: \":prefix=x\") {
  >     push(target:\"refs/heads/newbranch\", repo:\"real/repo2.git\")
  >   }
  > }"}
  > EOF
  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "push": true
      }
    }
  }
  $ git ls-remote --symref http://localhost:8001/real/repo2.git
  c90121689a90787dea0aa3be06701af6c66c3e20\trefs/heads/newbranch (esc)
  $ git ls-remote --symref http://localhost:8001/real_repo.git
  ref: refs/heads/master\tHEAD (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\tHEAD (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\trefs/heads/master (esc)
  bb282e9cdc1b972fffd08fd21eead43bc0c83cb8\trefs/heads/newbranch (esc)

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = ["::sub1/"]
  .
  |-- josh
  |   `-- cache
  |       `-- 26
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
  |   |   |-- 3d
  |   |   |   `-- 77ff51363c9825cc2a221fc0ba5a883a1a2c72
  |   |   |-- a0
  |   |   |   `-- 24003ee1acc6bf70318a46e7b6df651b9dc246
  |   |   |-- bb
  |   |   |   `-- 282e9cdc1b972fffd08fd21eead43bc0c83cb8
  |   |   |-- c8
  |   |   |   `-- 2fc150c43f13cc56c0e9caeba01b58ec612022
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
  |       |                   `-- newbranch
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 7e
      |   |   `-- 14e7e562164fb65d8f294303e07e915b95c5fe
      |   |-- c9
      |   |   `-- 0121689a90787dea0aa3be06701af6c66c3e20
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          `-- tags
  
  33 directories, 20 files

