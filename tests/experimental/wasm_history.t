  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1 sub2
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub2/file2
  $ git add sub1 sub2
  $ git commit -m "add subs" 1> /dev/null

Commit 1 introduces a module that selects sub1.

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
  $ git add st
  $ git commit -m "add module M1" 1> /dev/null

Commit 2 changes the module blob to select sub2 instead.

  $ cat > st/config.wasm <<'EOF'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  >   (memory (export "memory") 1)
  >   (data (i32.const 16) "sub2")
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  >   (func (export "josh_run") (result i32)
  >     (call $subdir (call $nop) (i32.const 16) (i32.const 4))))
  > EOF
  $ git add st
  $ git commit -m "switch module to M2" 1> /dev/null

Commit 3 touches content selected by the new module.

  $ echo changed > sub2/file2
  $ git add sub2
  $ git commit -m "change file2" 1> /dev/null

The module is resolved per parent, so the filter change between commit 1 and
commit 2 goes through the history splicing machinery: "switch module to M2"
becomes a synthetic merge splicing in the history of the newly selected sub2.

  $ josh-filter -s ':!st/config[::st/]' master --update refs/josh/master
  daf2882baa1fc025da6869cb3c46b726311e0b2f
  [1] :[
      ::st/
      :/sub1
  ]
  [1] :subtract[
          :/sub2
          :/sub1
      ]
  [4] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/config[::st/]
  ]
  [4] reachable_roots
  [4] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * change file2
  *   switch module to M2
  |\  
  | * add subs
  * add module M1
  * add subs

  $ git ls-tree -r refs/josh/master --name-only
  file2
  st/config.wasm

  $ git ls-tree -r refs/josh/master~1 --name-only
  file2
  st/config.wasm

  $ git ls-tree -r refs/josh/master~2 --name-only
  file1
  st/config.wasm
