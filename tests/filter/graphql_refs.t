  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1> /dev/null
  $ cd repo

  $ echo contents0 > file1
  $ git add .
  $ git commit -m "initial" 1> /dev/null

  $ git branch a/b
  $ git branch a-b
  $ git branch release-1.0
  $ git tag v1
  $ git symbolic-ref refs/heads/sym refs/heads/master

Default pattern "refs/heads/*": byte-sorted ("a-b" before "a/b"), symbolic "sym" skipped.

  $ josh-filter -g 'query { refs { name } }'
  35394071cd3b979e976016e08cddfc6fe16ca49b
  {
    "refs": [
      {
        "name": "refs/heads/a-b"
      },
      {
        "name": "refs/heads/a/b"
      },
      {
        "name": "refs/heads/master"
      },
      {
        "name": "refs/heads/release-1.0"
      }
    ]
  }

"*" matches across "/", like libgit2's wildmatch.

  $ josh-filter -g 'query { refs(pattern: "refs/heads/a*") { name } }'
  35394071cd3b979e976016e08cddfc6fe16ca49b
  {
    "refs": [
      {
        "name": "refs/heads/a-b"
      },
      {
        "name": "refs/heads/a/b"
      }
    ]
  }

A metacharacter mid-pattern: the part before it is the iteration prefix.

  $ josh-filter -g 'query { refs(pattern: "refs/heads/release-*.0") { name } }'
  35394071cd3b979e976016e08cddfc6fe16ca49b
  {
    "refs": [
      {
        "name": "refs/heads/release-1.0"
      }
    ]
  }

  $ josh-filter -g 'query { refs(pattern: "refs/tags/*") { name } }'
  35394071cd3b979e976016e08cddfc6fe16ca49b
  {
    "refs": [
      {
        "name": "refs/tags/v1"
      }
    ]
  }

A pattern the glob parser rejects (non-component "**") errors the field.

  $ josh-filter -g 'query { refs(pattern: "refs/h**") { name } }'
  35394071cd3b979e976016e08cddfc6fe16ca49b
  null
