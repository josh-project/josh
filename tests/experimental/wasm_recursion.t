  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

A self-referential module: it returns a filter that names ITSELF
(':!st/config[::st/config.wasm]'). Without the thread-local depth cap
(JOSH_WASM_MAX_DEPTH) resolving and applying the returned filter would
recurse without bound through resolve -> apply -> resolve.

  $ mkdir -p st sub1
  $ echo contents1 > sub1/file1
  $ cat > st/config.wasm <<'EOF'
  > (module
  >   (import "josh" "nop" (func $nop (result i32)))
  >   (import "josh" "file" (func $file (param i32 i32 i32) (result i32)))
  >   (import "josh" "wasm" (func $wasm (param i32 i32 i32 i32 i32 i32) (result i32)))
  >   (memory (export "memory") 1)
  >   (data (i32.const 16) "st/config")
  >   (data (i32.const 32) "st/config.wasm")
  >   (func (export "josh_abi_version") (result i32) (i32.const 1))
  >   (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  >   (func (export "josh_run") (result i32)
  >     (call $wasm
  >       (call $nop)
  >       (i32.const 16) (i32.const 9)
  >       (i32.const 0) (i32.const 0)
  >       (call $file (call $nop) (i32.const 32) (i32.const 14)))))
  > EOF
  $ git add st sub1
  $ git commit -m "add recursive module" 1> /dev/null

The command terminates promptly instead of overflowing the stack: the depth
cap makes the innermost nesting level degrade to an empty filter, and the
unwind then surfaces the deterministic "no invert" error (an `Op::Wasm`
returned by a module is not statically invertible, so the compose machinery
rejects it). The ref is not written.

  $ josh-filter -s ':!st/config[::st/config.wasm]' master --update refs/josh/master
  [1] reachable_roots
  [1] sequence_number
  ERROR: no invert Wasm("st/config", [], Chain([Subdir("st"), File("config.wasm", "config.wasm"), Prefix("st")]))
  [1]

  $ git ls-tree -r refs/josh/master --name-only
  fatal: Not a valid object name refs/josh/master
  [128]
