  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

message collision.

  $ git tag tag_a
  $ git tag tag_b

  $ josh-filter --squash-pattern "refs/tags/*" --update refs/heads/filtered
  62b5931076aa99843beabf8686ac8dcb7aecf130

  $ git log --pretty=%s refs/heads/filtered
  refs/tags/tag_a

like "*".

  $ josh-filter --squash-pattern "refs/tags/t**" --update refs/heads/filtered
  ERROR: Pattern syntax error near position 10: recursive wildcards must form a single path component
  [1]
