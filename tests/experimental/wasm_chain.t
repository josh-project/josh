  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

The module blob lives under sub/, NOT at the repository root.

  $ mkdir -p sub/st sub/inner other
  $ cat > sub/st/config.wasm <<'EOF'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  >   (memory (export "memory") 1)
  >   (data (i32.const 16) "inner")
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  >   (func (export "josh_run") (result i32)
  >     (call $subdir (call $nop) (i32.const 16) (i32.const 5))))
  > EOF
  $ echo contents1 > sub/inner/file1
  $ echo contents2 > other/file2
  $ git add sub other
  $ git commit -m "add tree" 1> /dev/null

In a chain the module is resolved from the tree at the op's position in the
pipeline: ':/sub' filters first, so 'st/config.wasm' is looked up in the
already-filtered tree.

  $ josh-filter -s ':/sub:!st/config[::st/]' master --update refs/josh/master
  ec564a064271c10e765d40ba436364a60ec34784
  [1] :/sub
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/config[::st/]
  ]
  [2] reachable_roots
  [2] sequence_number

  $ git ls-tree -r refs/josh/master --name-only
  file1
  st/config.wasm

The negative: the same op without the ':/sub' step looks the module up at the
repository root, where it does not exist. A missing module is an evaluation
error, so the projection is empty and no ref is written.

  $ josh-filter -s ':!st/config[::st/]' master --update refs/josh/negative
  Warning: reference refs/josh/negative wasn't updated
  0000000000000000000000000000000000000000
  [1] :/sub
  [2] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/config[::st/]
  ]
  [2] reachable_roots
  [2] sequence_number

  $ git ls-tree -r refs/josh/negative --name-only
  fatal: Not a valid object name refs/josh/negative
  [128]
