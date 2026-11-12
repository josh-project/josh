  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1 sub2 st
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub2/file2

This module selects ':/sub1' if the entry named by its first invocation
argument exists in the CONTEXT tree (checked via tree_entry_oid), and the
empty filter otherwise. It needs a real bump allocator: the argument string
must survive the allocation made for tree_entry_oid's result.

  $ cat > st/exists.wasm <<'EOF'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "empty" (func $empty (result i32)))
  >   (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  >   (import "josh" "invocation_args" (func $args (result i64)))
  >   (import "josh" "tree_entry_oid" (func $entry (param i32 i32) (result i64)))
  >   (memory (export "memory") 1)
  >   (data (i32.const 16) "sub1")
  >   (global $next (mut i32) (i32.const 4096))
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32)
  >     (local $p i32)
  >     (local.set $p (global.get $next))
  >     (global.set $next (i32.add (global.get $next) (local.get 0)))
  >     (local.get $p))
  >   (func $ptr (param i64) (result i32)
  >     (i32.wrap_i64 (i64.shr_u (local.get 0) (i64.const 32))))
  >   (func $len (param i64) (result i32)
  >     (i32.wrap_i64 (local.get 0)))
  >   (func (export "josh_run") (result i32)
  >     (local $a i64)
  >     (local.set $a (call $args))
  >     (if (result i32)
  >       (call $len (call $entry (call $ptr (local.get $a)) (call $len (local.get $a))))
  >       (then (call $subdir (call $nop) (i32.const 16) (i32.const 4)))
  >       (else (call $empty)))))
  > EOF

This module writes the hex OID of the entry named by its first argument into
a new file "oid.txt" via insert.

  $ cat > st/oid.wasm <<'EOF'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "insert" (func $insert (param i32 i32 i32 i32 i32) (result i32)))
  >   (import "josh" "invocation_args" (func $args (result i64)))
  >   (import "josh" "tree_entry_oid" (func $entry (param i32 i32) (result i64)))
  >   (memory (export "memory") 1)
  >   (data (i32.const 16) "oid.txt")
  >   (global $next (mut i32) (i32.const 4096))
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32)
  >     (local $p i32)
  >     (local.set $p (global.get $next))
  >     (global.set $next (i32.add (global.get $next) (local.get 0)))
  >     (local.get $p))
  >   (func $ptr (param i64) (result i32)
  >     (i32.wrap_i64 (i64.shr_u (local.get 0) (i64.const 32))))
  >   (func $len (param i64) (result i32)
  >     (i32.wrap_i64 (local.get 0)))
  >   (func (export "josh_run") (result i32)
  >     (local $a i64)
  >     (local $o i64)
  >     (local.set $a (call $args))
  >     (local.set $o (call $entry (call $ptr (local.get $a)) (call $len (local.get $a))))
  >     (call $insert (call $nop)
  >       (i32.const 16) (i32.const 7)
  >       (call $ptr (local.get $o)) (call $len (local.get $o)))))
  > EOF
  $ git add sub1 sub2 st
  $ git commit -m "add tree" 1> /dev/null

The argument names an entry inside the context (::st/ contains the module
blob itself), so the module returns ':/sub1' and file1 shows up at the root
of the projection, composed over the context.

  $ josh-filter -s ':!st/exists=st/exists.wasm[::st/]' master --update refs/josh/picked
  9ab25e932ebb76c1b376002b4ccbce86f7d97b97
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/exists=st/exists.wasm[::st/]
  ]
  [1] reachable_roots
  [1] sequence_number

  $ git ls-tree -r refs/josh/picked --name-only
  file1
  st/exists.wasm
  st/oid.wasm

The argument names an entry that exists in the repository but NOT in the
context tree: tree_entry_oid operates on the context-filtered tree, returns
"", and the module returns the empty filter — the projection is just the
context.

  $ josh-filter -s ':!st/exists=sub1[::st/]' master --update refs/josh/missed
  3c325aba0dab30d1b3a48898fe2df57ce3c3041c
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/exists=st/exists.wasm[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/exists=sub1[::st/]
  ]
  [1] reachable_roots
  [1] sequence_number

  $ git ls-tree -r refs/josh/missed --name-only
  st/exists.wasm
  st/oid.wasm

The oid module surfaces tree_entry_oid's result as file content: oid.txt must
contain exactly the blob OID of sub1/file1.

  $ josh-filter -s ':!st/oid=sub1/file1[:[::st/,::sub1/]]' master --update refs/josh/oid
  b92d3dcc359720f8a4a7dc3bf9f2e30752b860dd
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/exists=st/exists.wasm[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/exists=sub1[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/oid=sub1/file1[:[::st/,::sub1/]]
  ]
  [1] reachable_roots
  [1] sequence_number

  $ git show refs/josh/oid:oid.txt
  a024003ee1acc6bf70318a46e7b6df651b9dc246 (no-eol)

  $ git rev-parse master:sub1/file1
  a024003ee1acc6bf70318a46e7b6df651b9dc246
