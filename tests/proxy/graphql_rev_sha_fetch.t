  $ EXTRA_OPTS=--cache-duration\ 100 . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

The upstream must allow a fetch of a commit that no ref points to

  $ git config -f ${TESTTMP}/remote/real_repo.git/config uploadpack.allowAnySHA1InWant true

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir sub1
  $ echo c1 > sub1/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

Let the proxy fetch the repo once. The fetch starts the cache timer.

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { hash } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "hash": "00063fe9d1b5372b5878bead43f0eca7bbfbff67"
      }
    }
  }

Push a new commit upstream, without going through the proxy. Also push a commit
and then delete its branch, so that no upstream branch points to it.

  $ cd real_repo
  $ mkdir sub2
  $ echo c2 > sub2/file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null
  $ git push -q
  $ NEW_SHA=$(git rev-parse HEAD)

  $ git checkout -q -b tmp
  $ echo c3 > sub2/file3
  $ git add .
  $ git commit -m "add file3" 1> /dev/null
  $ git push -q origin tmp
  $ git push -q origin :tmp
  $ DANGLING_SHA=$(git rev-parse HEAD)
  $ git checkout -q master
  $ cd ${TESTTMP}

A query by ref name inside the cache duration does not fetch, like a git fetch by ref

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "hash": "00063fe9d1b5372b5878bead43f0eca7bbfbff67"
      }
    }
  }

A query by SHA for a commit that the mirror does not have fetches it, also inside the cache duration

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"${NEW_SHA}\", filter:\"::sub2/\") { files(at: \"\") { path text } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": [
          {
            "path": "sub2/file2",
            "text": "c2\n"
          }
        ]
      }
    }
  }

A commit that no upstream branch points to is fetched by SHA

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"${DANGLING_SHA}\", filter:\"::sub2/\") { files(at: \"\") { path } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": [
          {
            "path": "sub2/file2"
          },
          {
            "path": "sub2/file3"
          }
        ]
      }
    }
  }

A query by SHA for a commit that does not exist upstream fails with the fetch error

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"1111111111111111111111111111111111111111\") { hash } }"}
  > EOF

  $ cat ../query | curl -s -w "\n%{http_code}\n" -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git" | grep -o "not our ref [0-9a-f]*\|^[0-9]*$"
  not our ref 1111111111111111111111111111111111111111
  500

The fetches by SHA leave no refs in the mirror

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      "::sub1/",
      "::sub2/",
  ]
  .
  |-- josh
  |   `-- cache
  |       `-- 33
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
  |   |   |-- 00
  |   |   |   `-- 063fe9d1b5372b5878bead43f0eca7bbfbff67
  |   |   |-- 07
  |   |   |   `-- 71aea884dd394a7b12783d049f05b5599f41a4
  |   |   |-- 0d
  |   |   |   `-- da4231595f19bc13b4e854aff59c1918fefdca
  |   |   |-- 16
  |   |   |   |-- acc03a2dec56e17e7a92363be6f5d2fd09ef3e
  |   |   |   `-- f9ec009e5568c435f473ba3a1df732d49ce8c3
  |   |   |-- 75
  |   |   |   `-- b79546a6f715ff0967757b02a24890acffb019
  |   |   |-- 9a
  |   |   |   `-- c143d7601059d535c2e199b7f5ff616575e847
  |   |   |-- 9e
  |   |   |   `-- 5ea40974be7327ad6cf64a154ecb5afb688d76
  |   |   |-- ae
  |   |   |   `-- 9304576a6ec3419b231b2b9c8e33a06f97f9fb
  |   |   |-- ca
  |   |   |   `-- 01363f0bc2f4375b5cc7afcbc813a62f3378fe
  |   |   |-- dd
  |   |   |   `-- c0578bd52c4b8034cab3042430fa0f67dc79f6
  |   |   |-- fe
  |   |   |   `-- d002b009d30c1c8d49746b25aa84fce21633b0
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
      |   |-- info
      |   `-- pack
      |       |-- pack-675dc209908055bba2cfa14ac10032359f13b46e.idx
      |       `-- pack-675dc209908055bba2cfa14ac10032359f13b46e.pack
      `-- refs
          |-- heads
          `-- tags
  
  38 directories, 27 files
