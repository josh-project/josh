  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ mkdir -p a/b
  $ echo abdcontent > a/b/d

  $ mkdir sub1
  $ echo contents > sub1/file1
  $ git add .
  $ git commit -m "add file1"
  [master (root-commit) 1e64dc7] add file1
   2 files changed, 2 insertions(+)
   create mode 100644 a/b/d
   create mode 100644 sub1/file1

  $ git show HEAD
  commit 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file1
  
  diff --git a/a/b/d b/a/b/d
  new file mode 100644
  index 0000000..321f48c
  --- /dev/null
  +++ b/a/b/d
  @@ -0,0 +1 @@
  +abdcontent
  diff --git a/sub1/file1 b/sub1/file1
  new file mode 100644
  index 0000000..12f00e9
  --- /dev/null
  +++ b/sub1/file1
  @@ -0,0 +1 @@
  +contents

  $ tree
  .
  |-- a
  |   `-- b
  |       `-- d
  `-- sub1
      `-- file1
  
  3 directories, 2 files

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8002/real_repo.git full_repo

  $ cd full_repo

  $ tree
  .
  |-- a
  |   `-- b
  |       `-- d
  `-- sub1
      `-- file1
  
  3 directories, 2 files

  $ cat sub1/file1
  contents

  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   meta(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"foo\") {
  >     add(markers: [
  >      { path:\"a/b/c\", list: [{position:\"1234\",text:\"m1\"}]}
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "meta": {
        "add": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8002/real_repo.git@refs/josh/meta:nop.git
  From http://localhost:8002/real_repo.git@refs/josh/meta:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree f09b08b53a7f52de51c5cf6bba3da1c5e52f366d\tfoo (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- foo
      `-- markers
          `-- 1
              `-- e6
                  `-- 4dc
                      `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                          `-- a
                              `-- b
                                  `-- c
  
  8 directories, 1 file

  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   meta(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"foo\") {
  >     add(markers: [
  >      { path:\"a/b/d\", list: [{position:\"1235\",text:\"foobar\"}]}
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "meta": {
        "add": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8002/real_repo.git@refs/josh/meta:nop.git
  From http://localhost:8002/real_repo.git@refs/josh/meta:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree 626ccbfb5e3d317cc36be6627be099ed7103379b\tfoo (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- foo
      `-- markers
          `-- 1
              `-- e6
                  `-- 4dc
                      `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                          `-- a
                              `-- b
                                  |-- c
                                  `-- d
  
  8 directories, 2 files


  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   meta(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"foo\") {
  >     add(markers: [
  >      { path:\"a/b/d\", list: [
  >       {position:\"1235\",text:\"foobar\"},
  >       {position:\"1236\",text:\"foobar\"}
  >      ]}
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "meta": {
        "add": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8002/real_repo.git@refs/josh/meta:nop.git
  From http://localhost:8002/real_repo.git@refs/josh/meta:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree 1338bf3dceb3295bc7338da4ddad03464aaedb83\tfoo (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- foo
      `-- markers
          `-- 1
              `-- e6
                  `-- 4dc
                      `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                          `-- a
                              `-- b
                                  |-- c
                                  `-- d
  
  8 directories, 2 files

  $ cat > ../query <<EOF
  > {"query":"{ rev(at:\"refs/heads/master\") {
  >  files {
  >   path, text, markers(topic:\"foo\") {
  >     list {
  >       position, text
  >     }
  >     count
  >   }
  >  }
  >  dirs {
  >   path,markers(topic:\"foo\") {
  >     count
  >   }
  >  }
  > }}"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": [
          {
            "path": "a/b/d",
            "text": "abdcontent\n",
            "markers": {
              "list": [
                {
                  "position": "1235",
                  "text": "foobar"
                },
                {
                  "position": "1236",
                  "text": "foobar"
                }
              ],
              "count": 2
            }
          },
          {
            "path": "sub1/file1",
            "text": "contents\n",
            "markers": {
              "list": [],
              "count": 0
            }
          }
        ],
        "dirs": [
          {
            "path": "a",
            "markers": {
              "count": 3
            }
          },
          {
            "path": "a/b",
            "markers": {
              "count": 3
            }
          },
          {
            "path": "sub1",
            "markers": {
              "count": 0
            }
          }
        ]
      }
    }
  } (no-eol)

  $ cat > ../query <<EOF
  > {"query":"{ rev(at:\"refs/heads/master\", filter:\":/a\") {
  >  files {
  >   path, text, markers(topic:\"foo\") {
  >     list {
  >       position, text
  >     }
  >     count
  >   }
  >  }
  >  dirs {
  >   path,markers(topic:\"foo\") {
  >     count
  >   }
  >  }
  > }}"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": [
          {
            "path": "b/d",
            "text": "abdcontent\n",
            "markers": {
              "list": [
                {
                  "position": "1235",
                  "text": "foobar"
                },
                {
                  "position": "1236",
                  "text": "foobar"
                }
              ],
              "count": 2
            }
          }
        ],
        "dirs": [
          {
            "path": "b",
            "markers": {
              "count": 2
            }
          }
        ]
      }
    }
  } (no-eol)


  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [
      ':/a',
      ':/a/b',
      ':/sub1',
  ]
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       |-- %3A
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fa
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fa%2Fb
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3A%2Fsub1
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               |-- heads
  |               |   `-- master
  |               `-- josh
  |                   `-- meta
  |-- namespaces
  `-- tags
  
  19 directories, 6 files
