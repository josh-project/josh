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
  >   meta(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"tool/warn\", add: [
  >      { path:\"a/b/c\", data: [\"{\\\\\"location\\\\\":\\\\\"1234\\\\\",\\\\\"message\\\\\":\\\n \\\\\"m1\\\\\"}\"] }
  >   ])
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "meta": true
    }
  } (no-eol)

  $ git fetch ${TESTTMP}/remote/scratch refs/josh/upstream/real_repo.git/refs/josh/meta
  From /*/cramtests-*/markers.t/remote/scratch (glob)
   * branch            refs/josh/upstream/real_repo.git/refs/josh/meta -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker

  $ git diff ${EMPTY_TREE}..FETCH_HEAD
  diff --git a/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c
  new file mode 100644
  index 0000000..11474f8
  --- /dev/null
  +++ b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c
  @@ -0,0 +1 @@
  +43a0f340d27ea912af7a1cfbaa491cd117564a4e:{"location":"1234","message":"m1"}
  \ No newline at end of file

  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   meta(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"tool/warn\", add: [
  >      { path:\"a/b/c\", data: [\"{\\\\\"location\\\\\":\\\\\"1235\\\\\",\\\\\"message\\\\\":\\\\\"foobar\\\\\"}\"] }
  >   ])
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "meta": true
    }
  } (no-eol)

  $ git fetch ${TESTTMP}/remote/scratch refs/josh/upstream/real_repo.git/refs/josh/meta
  From /*/cramtests-*/markers.t/remote/scratch (glob)
   * branch            refs/josh/upstream/real_repo.git/refs/josh/meta -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker

  $ git diff ${EMPTY_TREE}..FETCH_HEAD
  diff --git a/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c
  new file mode 100644
  index 0000000..f81b303
  --- /dev/null
  +++ b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c
  @@ -0,0 +1,2 @@
  +43a0f340d27ea912af7a1cfbaa491cd117564a4e:{"location":"1234","message":"m1"}
  +c6058f73704cfe1879d4ef110910fc8b50ff04c7:{"location":"1235","message":"foobar"}
  \ No newline at end of file




  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   meta(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"tool/warn\", add: [
  >      { path:\"a/b/d\", data: [
  >        \"{\\\\\"location\\\\\":\\\\\"1235\\\\\",\\\\\"message\\\\\":\\\\\"foobar\\\\\"}\",
  >        \"{\\\\\"location\\\\\":\\\\\"1236\\\\\",\\\\\"message\\\\\":\\\\\"foobar\\\\\"}\"
  >      ]}
  >   ])
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "meta": true
    }
  } (no-eol)

  $ git fetch ${TESTTMP}/remote/scratch refs/josh/upstream/real_repo.git/refs/josh/meta
  From /*/cramtests-*/markers.t/remote/scratch (glob)
   * branch            refs/josh/upstream/real_repo.git/refs/josh/meta -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker
  * marker

  $ git diff ${EMPTY_TREE}..FETCH_HEAD
  diff --git a/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c
  new file mode 100644
  index 0000000..f81b303
  --- /dev/null
  +++ b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/c
  @@ -0,0 +1,2 @@
  +43a0f340d27ea912af7a1cfbaa491cd117564a4e:{"location":"1234","message":"m1"}
  +c6058f73704cfe1879d4ef110910fc8b50ff04c7:{"location":"1235","message":"foobar"}
  \ No newline at end of file
  diff --git a/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/d b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/d
  new file mode 100644
  index 0000000..249fd13
  --- /dev/null
  +++ b/tool/warn/~/1e/64d/c713/1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4/a/b/d
  @@ -0,0 +1,2 @@
  +53296c9e4dbc2b6ad15e15b2fc66870cd0548515:{"location":"1236","message":"foobar"}
  +c6058f73704cfe1879d4ef110910fc8b50ff04c7:{"location":"1235","message":"foobar"}
  \ No newline at end of file

  $ cat > ../query <<EOF
  > {"query":"{ rev(at:\"refs/heads/master\") {
  >  files {
  >   path, text, meta(topic:\"tool/warn\") {
  >     data {
  >       id
  >       message: string(at: \"/message\")
  >       location: string(at: \"/location\")
  >     }
  >     count
  >   }
  >  }
  >  dirs {
  >   path,meta(topic:\"tool/warn\") {
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
            "meta": {
              "data": [
                {
                  "id": "53296c9e4dbc2b6ad15e15b2fc66870cd0548515",
                  "message": "foobar",
                  "location": "1236"
                },
                {
                  "id": "c6058f73704cfe1879d4ef110910fc8b50ff04c7",
                  "message": "foobar",
                  "location": "1235"
                }
              ],
              "count": 2
            }
          },
          {
            "path": "sub1/file1",
            "text": "contents\n",
            "meta": {
              "data": [],
              "count": 0
            }
          }
        ],
        "dirs": [
          {
            "path": "a",
            "meta": {
              "count": 4
            }
          },
          {
            "path": "a/b",
            "meta": {
              "count": 4
            }
          },
          {
            "path": "sub1",
            "meta": {
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
  >   path, text, meta(topic:\"tool/warn\") {
  >     data {
  >       position: string(at: \"/location\"), text: string(at: \"message\")
  >     }
  >     count
  >   }
  >  }
  >  dirs {
  >   path,meta(topic:\"tool/warn\") {
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
            "meta": {
              "data": [
                {
                  "position": "1236",
                  "text": null
                },
                {
                  "position": "1235",
                  "text": null
                }
              ],
              "count": 2
            }
          }
        ],
        "dirs": [
          {
            "path": "b",
            "meta": {
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
  |   |       |-- %3A%2Fa
  |   |       |   `-- HEAD
  |   |       |-- %3A%2Fa%2Fb
  |   |       |   `-- HEAD
  |   |       `-- %3A%2Fsub1
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               |-- heads
  |               |   `-- master
  |               `-- josh
  |                   `-- meta
  |-- namespaces
  `-- tags
  
  14 directories, 6 files
