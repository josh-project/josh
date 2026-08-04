  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1 sub2
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub2/file2
  $ git add sub1 sub2
  $ git commit -m "add subs" 1> /dev/null

The script inspects the CONTEXT-filtered tree: sub2 exists in the repository
but is outside the context, so tree.files() cannot see it and no sub2 content
can end up in the result.

  $ mkdir -p tools st
  $ cp "$(wasm-guest-blob.sh starlark)" tools/starlark.wasm
  $ cat > st/config.star <<'EOF'
  > fs = [filter.file(f) for f in tree.files("sub1") + tree.files("sub2")]
  > filter = compose(fs)
  > EOF
  $ git add tools st
  $ git commit -m "add starlark config" 1> /dev/null

  $ josh-filter -s ':!tools/starlark=st/config.star[:[::st/,::sub1/]]' master --update refs/josh/master
  6f06692764a14121345345cefc8858ec449717af
  [1] :[
      ::st/
      sub1 = :/sub1:[
          :/
          ::file1
      ]
  ]
  [2] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark=st/config.star[:[::st/,::sub1/]]
  ]
  [2] reachable_roots
  [2] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * add starlark config
  * add subs

  $ git ls-tree -r refs/josh/master --name-only
  st/config.star
  sub1/file1
