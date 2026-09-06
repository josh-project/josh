# Experimental features

Experimental features are opt-in and must be enabled at runtime by setting the
environment variable `JOSH_EXPERIMENTAL_FEATURES=1`. Their behaviour or syntax
may change in future releases.

## Filters

### Object reference **`:&path`**
Reads the git object at `path` (a file or directory) and replaces it with a gitlink carrying
the object's ID. Although gitlinks normally identify submodule commits, this filter may store a
blob or tree ID in one so the reference does not require a separate pointer blob.

If `path` does not exist in the input tree, the filter is a no-op.

Example: `:&sub1` on a commit where `sub1` is a directory produces a gitlink `sub1` whose object
ID is the ID of that directory tree.

### Object dereference **`:#path`**
Reads the object ID stored directly in the gitlink at `path` and replaces the gitlink with the
object it identifies. Blob and tree IDs restore the corresponding file or directory written by
`:&path`. A commit ID is treated as a submodule reference: the commit's tree is restored at
`path`, and pointer updates merge the referenced commit history into the filtered history.
Changes made after a `:#path` submodule was inlined can be extracted with
`:/path:export`. The exported history is a fast-forward of the referenced
submodule commit and can be pushed upstream. Updating the superproject gitlink is a separate change.

If `path` does not exist the filter is a no-op. If the entry is not a gitlink or the referenced
object is not present in the repository, an error is returned.

Example: given a gitlink `sub1` carrying the ID of a directory tree or commit, `:#sub1` replaces
that gitlink with the actual directory tree at `sub1`.

### Object dereference into subdirectory **`:#/path`**
Dereferences the gitlink stored at `path` and then extracts the resulting object directly at the
repository root, discarding the `path` prefix. This is the typical way to restore content that
was previously stored with `:&path`.

Expands to `:#path:/path`. The canonical printed form is the expanded syntax.

Example: `:#/sub1` on a tree where `sub1` is a gitlink to a directory returns that directory's
contents at the root, as if `sub1` never existed.

### Tree ID capture **`:#path[filter]`**
Applies `filter` to the current tree and writes a gitlink carrying the resulting tree's object ID
at `path`. The filter itself does not appear in the output — only the reference it produces.

This lets you record a stable, content-addressed reference to a subtree alongside other entries.

Example: `:#version[:/sub1]` writes the ID of the `sub1` directory tree into the gitlink `version`.

### Wasm filter **`:!path=arg,...[context filter]`**
Runs a [WebAssembly](https://webassembly.org/) module stored in the repository and applies the
filter it returns. The module is the blob at `path` with a `.wasm` extension appended
automatically, looked up in the tree being filtered at the op's position in the pipeline (so in
`:/sub:!tools/mod`, the module is looked up in the already-`:/sub`-filtered tree).

The optional `=arg1,arg2,...` list passes invocation arguments to the module. Arguments are part
of the filter spec, not repo content; they lex like other filter-language arguments, so they
cannot contain commas, brackets or whitespace.

The optional `[context filter]` scopes the tree that is visible to the module: the context
filter is applied to the input tree first, and the result is the only tree the module can read.
The context filter does not affect the filter that the module returns — it only controls what
the module can see.

The output is `compose([context filter, returned filter])`. The module blob itself is *not*
forced into the output: it appears in the projection only if the context filter selects it.

Any evaluation failure — missing, oversized or invalid module, trap, fuel or memory exhaustion,
unsupported ABI version — degrades to the empty filter for that commit (trace-logged).

Evaluation is a pure function of `(module blob OID, invocation args, context-filtered tree
OID)`; results are memoized on that key, so a module runs when its *visible* input changes, not
once per commit. A narrow context filter is therefore also a performance tool.

**Resource limits**

Modules run sandboxed: no filesystem, network, clock or randomness access — only the `josh`
host imports listed below. Limits are configurable via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `JOSH_WASM_FUEL` | `100000000` | Instruction budget per evaluation. |
| `JOSH_WASM_MEMORY_LIMIT` | `67108864` | Guest memory limit in bytes (64 MiB). |
| `JOSH_WASM_MODULE_SIZE_LIMIT` | `16777216` | Maximum module blob size in bytes (16 MiB). |
| `JOSH_WASM_MODULE_CACHE_SIZE` | `64` | Compiled-module LRU cache capacity (entries). |
| `JOSH_WASM_MAX_DEPTH` | `10` | Nesting cap for wasm filters returning wasm filters. |
| `JOSH_WASM_MAX_HANDLES` | `100000` | Maximum filter handles constructed per evaluation. |

The limits are part of the persistent cache key for wasm filter results: changing a limit does
not serve results (including failure degradations) computed under the old configuration.

**Position in the filter expression**

`:!` works as the top-level op or as a chain element (with per-parent resolution and history
splicing, like `:workspace`), inside an `:exclude[...]`/`:select[...]` subfilter, and inside
`workspace.josh` or stored filter content (where it is legalized against a concrete tree).
Writing `:!` directly inside a compose group (e.g. `:[::other/,:!tools/mod]`) is not
supported: compose requires statically invertible parts and the wasm op has no static
inverse, so this fails deterministically with a `no invert` error. The same constraint
applies to a compose built by a module-returned filter that contains a nested wasm filter.

**Text format support**

If the blob at `path.wasm` does not start with the wasm binary magic and is valid UTF-8, it is
assembled as [WAT](https://webassembly.github.io/spec/core/text/index.html) text on the fly.
Small filters can stay human-readable in the repository:

```wat
(module
  (import "josh" "nop" (func $nop (result i32)))
  (import "josh" "subdir" (func $subdir (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "sub1")
  (func (export "josh_abi_version") (result i32) (i32.const 1))
  (func (export "josh_alloc") (param i32) (result i32) (i32.const 1024))
  (func (export "josh_run") (result i32)
    (call $subdir (call $nop) (i32.const 16) (i32.const 4))))
```

Applied with `:!st/config[::st/]` (the blob committed as `st/config.wasm`), this selects
`sub1` composed over the context.

**ABI (version 1)**

Plain core wasm, no WASI, no component model. Strings are UTF-8 `(ptr, len)` pairs in guest
memory; filters are opaque `u32` handles into a host-side table. Strings returned by the host
are written into a buffer obtained by calling the guest's exported `josh_alloc` and returned as
a packed `i64` (`ptr << 32 | len`; a zero length means the empty string and the pointer must
not be dereferenced).

The guest must export:

| Export | Type | Description |
|--------|------|-------------|
| `memory` | linear memory | The guest's memory. |
| `josh_abi_version` | `() -> i32` | Must return `1`. |
| `josh_alloc` | `(len: i32) -> i32` | Allocate a buffer for host-to-guest strings. Never freed. |
| `josh_run` | `() -> i32` | Entry point; returns the handle of the resulting filter. |

Host imports all live in module `"josh"`. The filter builder imports delegate to the native
filter constructors; `h` is a filter handle, `s` a string `(ptr, len)` pair, `S` a packed
host-to-guest string:

| Import | Signature | Description |
|--------|-----------|-------------|
| `nop` | `() -> h` | No-op filter. |
| `empty` | `() -> h` | Empty filter. |
| `chain` | `(a: h, b: h) -> h` | Apply `b` after `a`. |
| `compose` | `(ptr, count) -> h` | Overlay `count` little-endian `u32` handles read at `ptr`. |
| `subdir` | `(f: h, path: s) -> h` | Select a subdirectory and make it the root. |
| `prefix` | `(f: h, path: s) -> h` | Place the tree under a prefix. |
| `file` | `(f: h, path: s) -> h` | Select a single file. |
| `rename` | `(f: h, dst: s, src: s) -> h` | Select `src` and place it at `dst` (destination first). |
| `pattern` | `(f: h, glob: s) -> h` | Select paths matching a glob pattern. |
| `linear` | `(f: h) -> h` | Linearise history. |
| `workspace` | `(f: h, path: s) -> h` | Workspace filter rooted at `path`. |
| `stored` | `(f: h, path: s) -> h` | Stored filter at `path.josh`. |
| `author` | `(f: h, name: s, email: s) -> h` | Override the commit author. |
| `committer` | `(f: h, name: s, email: s) -> h` | Override the committer. |
| `message` | `(f: h, template: s) -> h` | Rewrite commit messages. |
| `unsign` | `(f: h) -> h` | Strip GPG signatures. |
| `prune_trivial_merge` | `(f: h) -> h` | Remove trivial merges. |
| `hook` | `(f: h, hook: s) -> h` | Apply a hook filter. |
| `with_meta` | `(f: h, key: s, value: s) -> h` | Attach metadata. |
| `insert` | `(f: h, path: s, content: s) -> h` | Insert a file with inline content. |
| `treeid` | `(f: h, path: s, sub: h) -> h` | Tree ID capture, like `:#path[sub]`. |
| `wasm` | `(f: h, path: s, args: s, ctx: h) -> h` | Nested wasm filter; `args` newline-joined. |
| `peel` | `(f: h) -> h` | Strip metadata from the filter. |
| `is_nop` | `(f: h) -> i32` | Returns 1 if the filter is a no-op. |

Tree access operates on the context-filtered tree; there is no way to read outside it:

| Import | Signature | Description |
|--------|-----------|-------------|
| `tree_file` | `(path: s) -> S` | Blob content at `path`; empty if absent, binary, not UTF-8 or larger than the memory limit. |
| `tree_files` | `(path: s) -> S` | Newline-joined full paths of files directly under `path`. |
| `tree_dirs` | `(path: s) -> S` | Newline-joined full paths of directories directly under `path`. |
| `tree_entry_oid` | `(path: s) -> S` | Hex OID of the entry at `path`; empty if absent. |
| `invocation_args` | `() -> S` | The newline-joined `=arg,...` list; empty if none. |

An invalid handle, a non-UTF-8 string or an out-of-bounds pointer traps and fails the
evaluation. Modules only link the imports they use, so adding builder imports in later josh
versions never breaks existing modules.

**Writing modules in Rust**

The `josh-filter-guest` crate (in `josh-guest/` in the josh repository) wraps the ABI in a
typed API: a `Filter` type with the builder methods, a `Tree` type for tree access, and an
entry-point macro. A module that mounts every top-level directory at its own name:

```rust
#![no_std]

use josh_filter_guest::{Filter, Tree, compose, josh_filter_entry, josh_guest_rt, nop};

fn run(tree: Tree) -> Filter {
    compose(
        tree.dirs("")
            .into_iter()
            .map(|d| nop().subdir(&d).prefix(&d)),
    )
}

josh_filter_entry!(run);
josh_guest_rt!();
```

Built with `cargo build --release --target wasm32-unknown-unknown`, this compiles to a
few-KiB `.wasm` blob. Committing the source next to the blob keeps reviews meaningful.

**Starlark scripts via the interpreter module**

Starlark filters are supported through a Starlark interpreter compiled to wasm
(`josh-starlark-guest`, built with `scripts/build-starlark-guest.sh` in the josh repository).
The interpreter blob is committed once per repository and takes the script path as its
invocation argument:

```
:!tools/starlark=st/config.star[::st/]
```

with the interpreter blob at `tools/starlark.wasm` and the script at `st/config.star`. The
script must be visible in the context-filtered tree — a missing or out-of-context script is a
deterministic evaluation error (resulting in the empty filter). Whether the script appears in
the projection is the author's choice, expressed through the context filter: above, the
script is part of the output while the interpreter blob is not.

**Script contract**

The script must assign a `Filter` value to the variable named `filter`. At the start of
execution `filter` is pre-set to a no-op filter, so a minimal script that selects nothing can
simply leave it unchanged, or assign a new value:

```python
filter = filter.subdir("src")
```

**Global variables available in the script**

| Variable | Type     | Description |
|----------|----------|-------------|
| `filter` | `Filter` | Starts as a no-op filter. Assign your result here. |
| `tree`   | `Tree`   | The context-filtered tree. |

**Global functions**

| Function | Description |
|----------|-------------|
| `compose([f1, f2, ...])` | Overlay multiple filters, same semantics as `:[f1,f2,...]`. |

**`Filter` methods**

All methods return a new `Filter` and can be chained.

| Method | Description |
|--------|-------------|
| `filter.subdir(path)` | Select a subdirectory and make it the root. |
| `filter.prefix(path)` | Place the tree under a subdirectory prefix. |
| `filter.file(path)` | Select a single file, keeping its path. |
| `filter.rename(dst, src)` | Select `src` and place it at `dst`. |
| `filter.pattern(pattern)` | Select files/directories matching a glob pattern (`*` allowed). |
| `filter.chain(other)` | Apply `other` after this filter. |
| `filter.nop()` | No-op; passes the tree through unchanged. |
| `filter.empty()` | Produce an empty tree. |
| `filter.linear()` | Linearise history (drop merge parents). |
| `filter.workspace(path)` | Apply the workspace filter rooted at `path`. |
| `filter.stored(path)` | Apply the stored filter at `path.josh`. |
| `filter.wasm(path, args, context_filter)` | Apply another wasm filter (replaces `filter.starlark(...)`). |
| `filter.author(name, email)` | Override the commit author. |
| `filter.committer(name, email)` | Override the committer. |
| `filter.message(template)` | Rewrite commit messages using a template. |
| `filter.unsign()` | Strip GPG signatures from commits. |
| `filter.prune_trivial_merge()` | Remove merge commits whose tree equals their first parent. |
| `filter.hook(hook)` | Apply a hook filter. |
| `filter.with_meta(key, value)` | Attach metadata to the filter. |
| `filter.insert(path, content)` | Insert a file with inline content. |
| `filter.treeid(path, subfilter)` | Tree ID capture, like `:#path[subfilter]`. |
| `filter.is_nop()` | Returns `True` if the filter is a no-op. |
| `filter.peel()` | Strip metadata from the filter. |

**`Tree` methods**

The `tree` object provides read-only access to the tree visible to the script.

| Method | Description |
|--------|-------------|
| `tree.file(path)` | Returns the text content of the file at `path`, or an empty string if absent or binary. |
| `tree.files(path)` | Returns a list of file paths that are direct children of `path`. |
| `tree.dirs(path)` | Returns a list of directory paths that are direct children of `path`. |
| `tree.tree(path)` | Returns a `Tree` object rooted at `path`. |

**Example**

A script that dynamically includes every top-level subdirectory as a prefixed subtree:

```python
# st/config.star
parts = [filter.subdir(d).prefix(d) for d in tree.dirs("")]
filter = compose(parts)
```

Applied with `:!tools/starlark=st/config.star[::st/]`.
