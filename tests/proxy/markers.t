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
  >   rev(at: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\") {
  >     meta(topic:\"tool/warn\", add: [
  >        { path:\"a/b/c\", data: [\"{\\\\\"location\\\\\":\\\\\"1234\\\\\",\\\\\"message\\\\\":\\\n \\\\\"m1\\\\\"}\"] }
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "meta": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8001/real_repo.git refs/josh/meta
  From http://localhost:8001/real_repo
   * branch            refs/josh/meta -> FETCH_HEAD

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
  >   rev(at: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\") {
  >     meta(topic:\"tool/warn\", add: [
  >        { path:\"a/b/c\", data: [\"{\\\\\"location\\\\\":\\\\\"1235\\\\\",\\\\\"message\\\\\":\\\\\"foobar\\\\\"}\"] }
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "meta": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8001/real_repo.git refs/josh/meta
  From http://localhost:8001/real_repo
   * branch            refs/josh/meta -> FETCH_HEAD

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
  >   rev(at: \"1e64dc7136eae9c6b88e4ab831322f3c72a5c0e4\") {
  >     meta(topic:\"tool/warn\", add: [
  >        { path:\"a/b/d\", data: [
  >          \"{\\\\\"location\\\\\":\\\\\"1235\\\\\",\\\\\"message\\\\\":\\\\\"foobar\\\\\"}\",
  >          \"{\\\\\"location\\\\\":\\\\\"1236\\\\\",\\\\\"message\\\\\":\\\\\"foobar\\\\\"}\"
  >        ]}
  >     ])
  >   }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "meta": true
      }
    }
  } (no-eol)

  $ git fetch http://localhost:8001/real_repo.git refs/josh/meta
  From http://localhost:8001/real_repo
   * branch            refs/josh/meta -> FETCH_HEAD

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
      "::a/",
      "::a/b/",
      "::sub1/",
  ]
  .
  |-- josh
  |   `-- 15
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
  |   |   |-- 00
  |   |   |   |-- 0fdb96141588097517f5361a52fd15f516236c
  |   |   |   `-- 398737066733a879fdeac01f830d2b7c2212c7
  |   |   |-- 10
  |   |   |   `-- e44181805d5aa02e9ae9d42f8e6f16a177712e
  |   |   |-- 11
  |   |   |   `-- 474f8beca288b120a723a57c7d201ddc07672f
  |   |   |-- 12
  |   |   |   |-- 1288d478f72ac23fd2f6fa9ef40dcafc0dac9f
  |   |   |   `-- f00e90b6ef79117ce6e650416b8cf517099b78
  |   |   |-- 13
  |   |   |   `-- 1043174b309a606a52c4f810cf509d4c120a69
  |   |   |-- 1e
  |   |   |   `-- 64dc7136eae9c6b88e4ab831322f3c72a5c0e4
  |   |   |-- 23
  |   |   |   `-- d5514a250d2387066895bc2575d329be7c9c38
  |   |   |-- 24
  |   |   |   |-- 9fd13d8f23051a218b48657ff1f2433de40b49
  |   |   |   `-- d379f3645131d769fe0e5de2d075a69417ea94
  |   |   |-- 32
  |   |   |   `-- 1f48cf6c921bde6a53891503bfdefad0040b59
  |   |   |-- 3d
  |   |   |   `-- d7fdbfe787ed80cb58a5905851a1840102e5f2
  |   |   |-- 3e
  |   |   |   `-- 4d66668e6f1dbadc079f36a84768a916bcb8f9
  |   |   |-- 3f
  |   |   |   `-- 724ed36477d2d9b4e849b85ce739be8f9ddd3e
  |   |   |-- 43
  |   |   |   `-- ba4ca11186832c55431d069bcc673a697ac081
  |   |   |-- 49
  |   |   |   `-- 0782f700bcfb460af23fd4804534b219f845a6
  |   |   |-- 6c
  |   |   |   `-- adbbeea146ce3172c8229e9d573e3eab7bdff1
  |   |   |-- 6f
  |   |   |   `-- a39dde3631ce9a1934bcbb94b838237769a44e
  |   |   |-- 77
  |   |   |   `-- cc643f01813fec7eaac6388bbe44886e2659bd
  |   |   |-- 8e
  |   |   |   |-- 1b4e9d32c8a6c0138a4316eb94c36c1e7de384
  |   |   |   `-- a81685295ad3ab09a9961a259982ae768d0e0e
  |   |   |-- 95
  |   |   |   |-- 2e0ed4fdd6cc6daf3d1c4fcb138fddc8ffdabb
  |   |   |   `-- c8826895fb97c2656fdc348d0dab5f943d9f7e
  |   |   |-- 98
  |   |   |   `-- 51c22e33a18b65ecc2dfcb07ac21ee53ecae8d
  |   |   |-- 9e
  |   |   |   `-- 6ec84b9177b428ce4a7343fca238e864cdf3d7
  |   |   |-- a2
  |   |   |   `-- b32d1c6c2e22ae44b3b3169ae14230d713c284
  |   |   |-- a9
  |   |   |   `-- 4835bf48c2916fbd6984c20564315e9c58660d
  |   |   |-- b0
  |   |   |   `-- f8e35251e96a874f6c597a2eb2f597b4d4aed2
  |   |   |-- b2
  |   |   |   `-- 8b2106a36e97a096e33a95825aff9f7d4860b5
  |   |   |-- b7
  |   |   |   `-- 39c5e88e64a494ccf6a0980788c2b2ef429f28
  |   |   |-- bf
  |   |   |   `-- 3ce158537f47e137717eb738930e95facdd300
  |   |   |-- ca
  |   |   |   `-- 8c6bc19dc6ec1ea4f60d4982b003edb4e26c26
  |   |   |-- cd
  |   |   |   `-- 7388ce64302b3a643177b76d6f0beac6779478
  |   |   |-- d1
  |   |   |   `-- 86c79c2a8e295d663e9fbd8dc65a33fc8bd160
  |   |   |-- d4
  |   |   |   `-- 407037b9a530bea6873143bbe61c7ad6d26feb
  |   |   |-- d6
  |   |   |   `-- 49338b5bec68500d266921a289a518b956452c
  |   |   |-- d7
  |   |   |   `-- ff5d05ada0212e6e904857429c406791b10c46
  |   |   |-- da
  |   |   |   `-- 26c060e8cf0ec0e99a9f851044cc2e76e245b4
  |   |   |-- db
  |   |   |   `-- 7ce571bb0c987e4e4aadbbd727867158c4a309
  |   |   |-- df
  |   |   |   `-- 0fb9ea1e23fb68a3c589d9b356b6a52bbd3c6f
  |   |   |-- e7
  |   |   |   `-- e7b082cc60acffc7991c438f6a03833bad2641
  |   |   |-- f8
  |   |   |   `-- 1b303f7a6f22739b9513836d934f622243c15a
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       |-- josh
  |       |   `-- upstream
  |       |       `-- real_repo.git
  |       |           |-- HEAD
  |       |           `-- refs
  |       |               |-- heads
  |       |               |   `-- master
  |       |               `-- josh
  |       |                   `-- meta
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- 00
      |   |   |-- 0fdb96141588097517f5361a52fd15f516236c
      |   |   `-- 398737066733a879fdeac01f830d2b7c2212c7
      |   |-- 10
      |   |   `-- e44181805d5aa02e9ae9d42f8e6f16a177712e
      |   |-- 11
      |   |   `-- 474f8beca288b120a723a57c7d201ddc07672f
      |   |-- 12
      |   |   `-- 1288d478f72ac23fd2f6fa9ef40dcafc0dac9f
      |   |-- 13
      |   |   `-- 1043174b309a606a52c4f810cf509d4c120a69
      |   |-- 18
      |   |   `-- c7ee0bdd23edd6563b453e8773f3815f45022d
      |   |-- 23
      |   |   |-- 87c59576f220507ff0e0de060ceacd71ff41a3
      |   |   `-- d5514a250d2387066895bc2575d329be7c9c38
      |   |-- 24
      |   |   |-- 9fd13d8f23051a218b48657ff1f2433de40b49
      |   |   `-- d379f3645131d769fe0e5de2d075a69417ea94
      |   |-- 30
      |   |   `-- bad8e97384f1ac8cbccd35304d85c9e12b5665
      |   |-- 3d
      |   |   `-- d7fdbfe787ed80cb58a5905851a1840102e5f2
      |   |-- 43
      |   |   `-- ba4ca11186832c55431d069bcc673a697ac081
      |   |-- 49
      |   |   `-- 0782f700bcfb460af23fd4804534b219f845a6
      |   |-- 5b
      |   |   `-- c77f5c5a22619575241e72415aac832204e714
      |   |-- 61
      |   |   `-- 3cf076586b0dec8f8547b5bfd27f4327e2e489
      |   |-- 62
      |   |   `-- 4b4ae13c87b02e347717cedff72da5e175c689
      |   |-- 6c
      |   |   `-- adbbeea146ce3172c8229e9d573e3eab7bdff1
      |   |-- 6f
      |   |   `-- a39dde3631ce9a1934bcbb94b838237769a44e
      |   |-- 77
      |   |   `-- cc643f01813fec7eaac6388bbe44886e2659bd
      |   |-- 8e
      |   |   `-- 1b4e9d32c8a6c0138a4316eb94c36c1e7de384
      |   |-- 91
      |   |   `-- 509498b7b323626ac259fae13f6c50eea5c947
      |   |-- 95
      |   |   |-- 2e0ed4fdd6cc6daf3d1c4fcb138fddc8ffdabb
      |   |   `-- c8826895fb97c2656fdc348d0dab5f943d9f7e
      |   |-- 98
      |   |   `-- 51c22e33a18b65ecc2dfcb07ac21ee53ecae8d
      |   |-- 9e
      |   |   `-- 6ec84b9177b428ce4a7343fca238e864cdf3d7
      |   |-- a2
      |   |   `-- b32d1c6c2e22ae44b3b3169ae14230d713c284
      |   |-- a9
      |   |   `-- 4835bf48c2916fbd6984c20564315e9c58660d
      |   |-- b0
      |   |   |-- d4426b7d7d8157600c990d18b62e8a237bffa6
      |   |   `-- f8e35251e96a874f6c597a2eb2f597b4d4aed2
      |   |-- b1
      |   |   `-- d540fbdaef90043e873b454fca330ec20d57f3
      |   |-- b2
      |   |   `-- 8b2106a36e97a096e33a95825aff9f7d4860b5
      |   |-- b5
      |   |   `-- 2a6b1f3afba295c5463036ce7dd3f2a675b915
      |   |-- b7
      |   |   `-- 39c5e88e64a494ccf6a0980788c2b2ef429f28
      |   |-- bf
      |   |   `-- 3ce158537f47e137717eb738930e95facdd300
      |   |-- ca
      |   |   `-- 8c6bc19dc6ec1ea4f60d4982b003edb4e26c26
      |   |-- cd
      |   |   `-- 7388ce64302b3a643177b76d6f0beac6779478
      |   |-- d1
      |   |   `-- 86c79c2a8e295d663e9fbd8dc65a33fc8bd160
      |   |-- d4
      |   |   `-- 407037b9a530bea6873143bbe61c7ad6d26feb
      |   |-- d6
      |   |   `-- 49338b5bec68500d266921a289a518b956452c
      |   |-- d7
      |   |   |-- 15747fe0dcdc8e0a2e57d2c8bfcbc5ddfe1544
      |   |   `-- ff5d05ada0212e6e904857429c406791b10c46
      |   |-- db
      |   |   `-- 7ce571bb0c987e4e4aadbbd727867158c4a309
      |   |-- df
      |   |   `-- 0fb9ea1e23fb68a3c589d9b356b6a52bbd3c6f
      |   |-- e0
      |   |   `-- 77d6ab1d2b2335e25b879c22c32fb3d9d64188
      |   |-- e7
      |   |   `-- e7b082cc60acffc7991c438f6a03833bad2641
      |   |-- ed
      |   |   `-- fdf0b26b57e156cb1f118a633af077d9ba128a
      |   |-- f8
      |   |   `-- 1b303f7a6f22739b9513836d934f622243c15a
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          |-- namespaces
          `-- tags
  
  108 directories, 106 files
