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
  >   rev(at: "HEAD", filter: ":workspace=ws") {
  >     history(limit: 1) {
  >       summary
  >       message
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
          "message": "add ws\n"
        }
      ]
    }
  }

  $ cat > query2 <<EOF
  > query {
  >   rev(at: "HEAD", filter: ":workspace=ws") {
  >     history(limit: 2) {
  >       summary
  >       authorEmail
  >     }
  >   }
  > }
  > EOF

  $ josh-filter -g "$(cat query2)"
  {
    "rev": {
      "history": [
        {
          "summary": "add ws",
          "authorEmail": "josh@example.com"
        }
      ]
    }
  }
