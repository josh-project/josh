  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir sub1 sub2
  $ echo c1 > sub1/file1
  $ echo c2 > sub1/test
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ echo c3 > sub2/file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir sub2/nested
  $ echo c4 > sub2/nested/file3
  $ git add .
  $ git commit -m "third commit subject" -m "and a body line" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

History is the filtered commit's first-parent chain, windowed by limit and offset

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { history(limit: 2) { hash summary } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "history": [
          {
            "hash": "649cd08b3651125286542f5f3d254a32e398c6a2",
            "summary": "third commit subject"
          },
          {
            "hash": "0330ee19fb76a77a368600634c10a17dd0dc9343",
            "summary": "add file2"
          }
        ]
      }
    }
  }

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { history(limit: 2, offset: 1) { hash summary } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "history": [
          {
            "hash": "0330ee19fb76a77a368600634c10a17dd0dc9343",
            "summary": "add file2"
          },
          {
            "hash": "c71731c57cf0d21a79a3ff2f297ee69904054931",
            "summary": "add file1"
          }
        ]
      }
    }
  }

A limit past the end of the chain stops at the root commit

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { history(limit: 99) { summary } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "history": [
          {
            "summary": "third commit subject"
          },
          {
            "summary": "add file2"
          },
          {
            "summary": "add file1"
          }
        ]
      }
    }
  }

Commit fields come from the filtered commit

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { summary message authorEmail date(format: \"%Y-%m-%d %H:%M:%S\") } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "summary": "third commit subject",
        "message": "third commit subject\n\nand a body line\n",
        "authorEmail": "josh@example.com",
        "date": "2005-04-07 22:13:13"
      }
    }
  }

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\", filter:\"::sub1/\") { summary authorEmail history(limit: 2) { summary } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "summary": "add file1",
        "authorEmail": "josh@example.com",
        "history": [
          {
            "summary": "add file1"
          }
        ]
      }
    }
  }

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { parents { summary } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "parents": [
          {
            "summary": "add file2"
          }
        ]
      }
    }
  }

A filter that matches nothing is reported as a warning

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\", filter:\"::nonexistent/\") { warnings { message } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "warnings": [
          {
            "message": "No match for \"::nonexistent/\""
          }
        ]
      }
    }
  }

Listings are depth-limited, and directories exclude blobs

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { files { path } dirs { path } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": [
          {
            "path": "sub1/file1"
          },
          {
            "path": "sub1/test"
          },
          {
            "path": "sub2/file2"
          },
          {
            "path": "sub2/nested/file3"
          }
        ],
        "dirs": [
          {
            "path": "sub1"
          },
          {
            "path": "sub2"
          },
          {
            "path": "sub2/nested"
          }
        ]
      }
    }
  }

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { files(depth: 1) { path } dirs(depth: 1) { path } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": [],
        "dirs": [
          {
            "path": "sub1"
          },
          {
            "path": "sub2"
          }
        ]
      }
    }
  }

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { files(at:\"sub2\") { path } } }"}
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
            "path": "sub2/nested/file3"
          }
        ]
      }
    }
  }

Listing a path that is missing, or is a file, is an error

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { files(at:\"nope\") { path } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": null
      }
    },
    "errors": [
      {
        "message": "no such path: nope",
        "locations": [
          {
            "line": 1,
            "column": 33
          }
        ],
        "path": [
          "rev",
          "files"
        ]
      }
    ]
  }

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { files(at:\"sub1/file1\") { path } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "files": null
      }
    },
    "errors": [
      {
        "message": "not a directory: sub1/file1",
        "locations": [
          {
            "line": 1,
            "column": 33
          }
        ],
        "path": [
          "rev",
          "files"
        ]
      }
    ]
  }

Blob contents, and a path that does not exist

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { file(path:\"sub1/file1\") { path text } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "file": {
          "path": "sub1/file1",
          "text": "c1\n"
        }
      }
    }
  }

  $ cat > ../query << EOF
  > {"query": "{ rev(at:\"refs/heads/master\") { file(path:\"nope/missing\") { path } } }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "file": null
      }
    }
  }
