  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1> /dev/null
  $ cd repo

  $ echo contents0 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null
  $ echo contents2 > file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null
  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > ::file2
  > EOF
  $ git add .
  $ git commit -m "add ws" 1> /dev/null

  $ cat > query <<EOF
  > query {
  >   rev(at: "HEAD") {
  >     history(limit: 10) {
  >       summary
  >       changedFiles {
  >         from { path }
  >         to { path }
  >       }
  >     }
  >   }
  > }
  > EOF

  $ josh-filter -g "$(cat query)"
  {
    "rev": {
      "history": [
        {
          "summary": "add ws",
          "changedFiles": [
            {
              "from": null,
              "to": {
                "path": "ws/workspace.josh"
              }
            }
          ]
        },
        {
          "summary": "add file2",
          "changedFiles": [
            {
              "from": null,
              "to": {
                "path": "file2"
              }
            }
          ]
        },
        {
          "summary": "add file1",
          "changedFiles": [
            {
              "from": null,
              "to": {
                "path": "file1"
              }
            }
          ]
        }
      ]
    }
  }
