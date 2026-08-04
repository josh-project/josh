  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add sub1" 1> /dev/null

  $ mkdir sub2
  $ echo contents2 > sub2/file2
  $ git add sub2
  $ git commit -m "add sub2" 1> /dev/null

A wasm module can be committed as WAT text; it is assembled on the fly.
This one always selects sub1.

  $ mkdir -p st
  $ cat > st/config.wasm <<'EOF'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  >   (memory (export "memory") 1)
  >   (data (i32.const 16) "sub1")
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  >   (func (export "josh_run") (result i32)
  >     (call $subdir (call $nop) (i32.const 16) (i32.const 4))))
  > EOF

This one selects the subdir named by its first invocation argument.

  $ cat > st/pick.wasm <<'EOF'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  >   (import "josh" "invocation_args" (func $args (result i64)))
  >   (memory (export "memory") 1)
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  >   (func (export "josh_run") (result i32)
  >     (local $packed i64)
  >     (local.set $packed (call $args))
  >     (call $subdir (call $nop)
  >       (i32.wrap_i64 (i64.shr_u (local.get $packed) (i64.const 32)))
  >       (i32.wrap_i64 (i64.and (local.get $packed) (i64.const 0xffffffff))))))
  > EOF
  $ git add st
  $ git commit -m "add wasm config" 1> /dev/null

The output is the composition of the context (::st/) and the filter the module
returns. The module blob is only included because the context selects it.
Commits that predate the module filter to empty (a missing module is an
evaluation error), so "add sub2" collapses away.

  $ josh-filter -s ':!st/config[::st/]' master --update refs/josh/master
  645068e4acfe7ecad0af8ed8ba46d9ca42a15639
  [1] :[
      ::st/
      :/sub1
  ]
  [2] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/config[::st/]
  ]
  [3] reachable_roots
  [3] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * add wasm config
  * add sub1

  $ git ls-tree -r refs/josh/master --name-only
  file1
  st/config.wasm
  st/pick.wasm

Invocation args: the same history filtered through the arg-driven module.

  $ josh-filter -s ':!st/pick=sub2[::st/]' master --update refs/josh/args
  6f90323c57721a54c05d34eeaaf9239df4873432
  [1] :[
      ::st/
      :/sub1
  ]
  [2] :[
      ::st/
      :/sub2
  ]
  [2] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/config[::st/]
  ]
  [2] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/pick=sub2[::st/]
  ]
  [3] reachable_roots
  [3] sequence_number

  $ git ls-tree -r refs/josh/args --name-only
  file2
  st/config.wasm
  st/pick.wasm
