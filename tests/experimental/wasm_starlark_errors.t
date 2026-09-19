  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1 other
  $ echo contents1 > sub1/file1
  $ echo contents2 > other/file2
  $ mkdir -p tools st
  $ cp "$(wasm-guest-blob.sh starlark)" tools/starlark.wasm
  $ cat > st/config.star <<'EOF'
  > filter = filter.subdir("sub1")
  > EOF
  $ git add sub1 other tools st
  $ git commit -m "add tree" 1> /dev/null

A missing or out-of-context script is a deterministic evaluation error, not a
nop: the interpreter checks the script's presence in the context tree via
tree_entry_oid and traps when it is absent. Every case below degrades to the
empty projection — zero OID, ref not written.

The script exists in the repository but the context excludes it:

  $ josh-filter -s ':!tools/starlark=st/config.star[::other/]' master --update refs/josh/outofcontext
  Warning: reference refs/josh/outofcontext wasn't updated
  0000000000000000000000000000000000000000
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark=st/config.star[::other/]
  ]
  [1] reachable_roots
  [1] sequence_number

The script does not exist at all:

  $ josh-filter -s ':!tools/starlark=st/missing.star[::st/]' master --update refs/josh/missing
  Warning: reference refs/josh/missing wasn't updated
  0000000000000000000000000000000000000000
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark=st/config.star[::other/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark=st/missing.star[::st/]
  ]
  [1] reachable_roots
  [1] sequence_number

No script argument given at all:

  $ josh-filter -s ':!tools/starlark[::st/]' master --update refs/josh/noarg
  Warning: reference refs/josh/noarg wasn't updated
  0000000000000000000000000000000000000000
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark=st/config.star[::other/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark=st/missing.star[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark[::st/]
  ]
  [1] reachable_roots
  [1] sequence_number

  $ git ls-tree -r refs/josh/outofcontext --name-only
  fatal: Not a valid object name refs/josh/outofcontext
  [128]
