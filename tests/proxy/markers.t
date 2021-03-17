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

  $ git fetch http://localhost:8002/real_repo.git@refs/josh/meta/foo:nop.git
  From http://localhost:8002/real_repo.git@refs/josh/meta/foo:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree 3a2db85a05033dc4667f118580ca301c95be164a\t1 (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- 1
      `-- e6
          `-- 4dc
              `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                  `-- markers
                      `-- a
                          `-- b
                              `-- c
  
  7 directories, 1 file

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

  $ git fetch http://localhost:8002/real_repo.git@refs/josh/meta/foo:nop.git
  From http://localhost:8002/real_repo.git@refs/josh/meta/foo:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree 527de5a876f9fbe2d9ed410c5f52da0854d47386\t1 (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- 1
      `-- e6
          `-- 4dc
              `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                  `-- markers
                      `-- a
                          `-- b
                              |-- c
                              `-- d
  
  7 directories, 2 files


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

  $ git fetch http://localhost:8002/real_repo.git@refs/josh/meta/foo:nop.git
  From http://localhost:8002/real_repo.git@refs/josh/meta/foo:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree 0d733c8f9fd482a75d99887e87e0a57344e903bb\t1 (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- 1
      `-- e6
          `-- 4dc
              `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                  `-- markers
                      `-- a
                          `-- b
                              |-- c
                              `-- d
  
  7 directories, 2 files

  $ cat > ../query <<EOF
  > {"query":"{ rev(at:\"refs/heads/master\") {
  >  files {
  >   path, text, markers(topic:\"foo\") { list {
  >     position, text
  >   }}
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
              ]
            }
          },
          {
            "path": "sub1/file1",
            "text": "contents\n",
            "markers": {
              "list": []
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
  |                       `-- foo
  |-- namespaces
  `-- tags
  
  20 directories, 6 files
