  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1 st
  $ echo contents1 > sub1/file1

A blob that is neither wasm nor valid WAT text.

  $ cat > st/broken.wasm <<'EOF'
  > (module this is not a valid module (((
  > EOF

A module that burns fuel forever: an infinite loop in josh_run. The fuel
limit turns it into a deterministic evaluation error instead of a hang.

  $ cat > st/fuel.wasm <<'EOF'
  > (module
  >   (memory (export "memory") 1)
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  >   (func (export "josh_run") (result i32)
  >     (loop $l (br $l))
  >     (i32.const 0)))
  > EOF
  $ git add sub1 st
  $ git commit -m "add tree" 1> /dev/null

Any evaluation failure degrades to the empty projection: the ref is not
written and the resulting OID is zero.

A broken module:

  $ josh-filter -s ':!st/broken[::st/]' master --update refs/josh/broken
  Warning: reference refs/josh/broken wasn't updated
  0000000000000000000000000000000000000000
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/broken[::st/]
  ]
  [1] reachable_roots
  [1] sequence_number

A missing module blob:

  $ josh-filter -s ':!st/missing[::st/]' master --update refs/josh/missing
  Warning: reference refs/josh/missing wasn't updated
  0000000000000000000000000000000000000000
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/broken[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/missing[::st/]
  ]
  [1] reachable_roots
  [1] sequence_number

The fuel bomb (terminates promptly by running out of fuel):

  $ josh-filter -s ':!st/fuel[::st/]' master --update refs/josh/fuel
  Warning: reference refs/josh/fuel wasn't updated
  0000000000000000000000000000000000000000
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/broken[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/fuel[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/missing[::st/]
  ]
  [1] reachable_roots
  [1] sequence_number

  $ git ls-tree -r refs/josh/broken --name-only
  fatal: Not a valid object name refs/josh/broken
  [128]

A wasm filter cannot be written directly inside a compose group: compose
requires statically invertible parts and the wasm op has no static inverse,
so this fails deterministically instead of degrading. A valid module makes
no difference.

  $ cat > st/valid.wasm <<'EOF2'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  >   (memory (export "memory") 1)
  >   (data (i32.const 16) "sub1")
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  >   (func (export "josh_run") (result i32)
  >     (call $subdir (call $nop) (i32.const 16) (i32.const 4))))
  > EOF2
  $ git add st
  $ git commit -m "add valid module" 1> /dev/null

  $ josh-filter -s ':[::sub1/,:!st/valid[::st/]]' master --update refs/josh/compose
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/broken[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/fuel[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/missing[::st/]
  ]
  [2] reachable_roots
  [2] sequence_number
  ERROR: no invert Wasm("st/valid", [], Chain([Subdir("st"), Prefix("st")]))
  [1]

The same op inside an exclude subfilter is supported.

  $ josh-filter -s ':exclude[:!st/valid[::st/]]' master --update refs/josh/exclude
  73959a47b0dffea09d8c55ac0c95133a12981335
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/broken[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/fuel[::st/]
  ]
  [1] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!st/missing[::st/]
  ]
  [2] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :exclude[:!st/valid[::st/]]
  ]
  [2] reachable_roots
  [2] sequence_number
