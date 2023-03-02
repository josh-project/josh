  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1> /dev/null
  $ cd repo

  $ echo contents0 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ cat > query <<EOF
  > query {
  >   rev(at: "HEAD") {
  >     describe {
  >       name
  >       depth
  >     }
  >   }
  > }
  > EOF

  $ josh-filter -g "$(cat query)"
  {
    "rev": {
      "describe": {
        "name": null,
        "depth": 0
      }
    }
  }

  $ git tag v0.1.0
  $ josh-filter -g "$(cat query)"
  {
    "rev": {
      "describe": {
        "name": "v0.1.0",
        "depth": 0
      }
    }
  }

  $ echo contents2 > file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null

  $ josh-filter -g "$(cat query)"
  {
    "rev": {
      "describe": {
        "name": "v0.1.0",
        "depth": 1
      }
    }
  }

  $ HEAD=$(git rev-parse HEAD)
  $ cat > query <<EOF
  > query {
  >   rev(at: "${HEAD}") {
  >     describe {
  >       name
  >       depth
  >     }
  >   }
  > }
  > EOF

  $ josh-filter -g "$(cat query)"
  {
    "rev": {
      "describe": {
        "name": "v0.1.0",
        "depth": 1
      }
    }
  }
