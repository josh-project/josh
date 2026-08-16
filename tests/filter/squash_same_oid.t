  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

Two tags on one commit: refs are enumerated byte-sorted and a duplicate target oid
takes the last-inserted message filter, so the byte-greatest refname wins the
message collision.

  $ git tag tag_a
  $ git tag tag_b

  $ josh-filter --squash-pattern "refs/tags/*" --update refs/heads/filtered
  077b2cad7b3fbc393b6320b90c9c0be1255ac309

  $ git log --pretty=%s refs/heads/filtered
  refs/tags/tag_b

A pattern the glob parser rejects (non-component "**") errors instead of matching
like "*".

  $ josh-filter --squash-pattern "refs/tags/t**" --update refs/heads/filtered
  ERROR: Pattern syntax error near position 10: recursive wildcards must form a single path component
  [1]
