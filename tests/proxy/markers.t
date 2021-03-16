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
  >   metadata(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"foo\") {
  >     add(comments: [
  >      { path:\"a/b/c\", markers: [{position:\"1234\",text:\"m1\"}]}
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "metadata": {
        "add": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8002/real_repo.git@refs/metadata/foo:nop.git
  From http://localhost:8002/real_repo.git@refs/metadata/foo:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree cdbd6c96689b4c14ba07b41273cfbd24de9e2c4f\t1 (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- 1
      `-- e6
          `-- 4dc
              `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                  `-- a
                      `-- b
                          `-- c
  
  6 directories, 1 file

  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   metadata(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"foo\") {
  >     add(comments: [
  >      { path:\"a/b/d\", markers: [{position:\"1235\",text:\"foobar\"}]}
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "metadata": {
        "add": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8002/real_repo.git@refs/metadata/foo:nop.git
  From http://localhost:8002/real_repo.git@refs/metadata/foo:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree 8e2f931abcf2dc6058db4c81e97817b8e4b5ce6b\t1 (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- 1
      `-- e6
          `-- 4dc
              `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                  `-- a
                      `-- b
                          |-- c
                          `-- d
  
  6 directories, 2 files


  $ cat > ../query <<EOF
  > {"query":"mutation {
  >   metadata(commit: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\", topic:\"foo\") {
  >     add(comments: [
  >      { path:\"a/b/d\", markers: [
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
      "metadata": {
        "add": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8002/real_repo.git@refs/metadata/foo:nop.git
  From http://localhost:8002/real_repo.git@refs/metadata/foo:nop
   * branch            HEAD       -> FETCH_HEAD

  $ git log --graph --pretty=%s FETCH_HEAD
  * marker
  * marker
  * marker

  $ git ls-tree FETCH_HEAD
  040000 tree bc163abfb381f8f347805b4353a29e6ca11ec6ae\t1 (esc)

  $ git checkout -q FETCH_HEAD

  $ tree
  .
  `-- 1
      `-- e6
          `-- 4dc
              `-- 1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4
                  `-- a
                      `-- b
                          |-- c
                          `-- d
  
  6 directories, 2 files

  $ cat > ../query <<EOF
  > {"query":"{ rev(at:\"refs/heads/master\") { files { path, text, comments(topic:\"foo\") {
  > position, text } } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": [
          {
            "path": "a/b/d",
            "text": "abdcontent\n",
            "comments": [
              {
                "position": "1235",
                "text": "foobar"
              },
              {
                "position": "1236",
                "text": "foobar"
              }
            ]
          },
          {
            "path": "sub1/file1",
            "text": "contents\n",
            "comments": []
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
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fa%2Fb
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       |-- %3A%2Fsub1
  |   |       |   `-- heads
  |   |       |       `-- master
  |   |       `-- %3Anop
  |   |           `-- heads
  |   |               `-- master
  |   `-- upstream
  |       `-- real_repo.git
  |           `-- refs
  |               |-- heads
  |               |   `-- master
  |               `-- metadata
  |                   `-- foo
  |-- namespaces
  `-- tags
  
  19 directories, 6 files
