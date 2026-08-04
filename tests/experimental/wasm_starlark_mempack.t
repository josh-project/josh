  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add sub1" 1> /dev/null

  $ mkdir tools
  $ cp "$(wasm-guest-blob.sh starlark)" tools/starlark.wasm
  $ cat > config.star <<'EOF'
  > # The context (:/sub1::file1:prefix=lib) produces a tree that is written to
  > # the mempack backend when running with --pack. This test verifies that the
  > # wasm evaluation can access that mempack tree (the transaction repo must be
  > # used directly, not reopened).
  > content = tree.file("lib/file1")
  > filter = filter.insert("foobar", content)
  > EOF

Apply with --pack: the script must see the mempack tree to return the correct
filter.

  $ josh-filter --pack ':!tools/starlark=config.star[:[::config.star,:/sub1::file1:prefix=lib]]' . --update refs/josh/master
  30527e47358a1b726b5d9bec51f906709b2c65a7

  $ git ls-tree -r refs/josh/master --name-only
  config.star
  foobar
  lib/file1

The script read "lib/file1" from the mempack tree and inserted its content as
"foobar" at the root of the result.

  $ git show refs/josh/master:foobar
  contents1
