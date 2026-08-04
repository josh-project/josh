# josh-starlark-guest

A Starlark interpreter compiled to `wasm32-unknown-unknown`, usable as an
ordinary josh wasm filter guest module. It provides the same script API as
the old native `josh-starlark` filter (pre-set `filter` and `tree` module
variables, global `compose()`), with `filter.starlark(path, sub)` replaced by
`filter.wasm(path, args, sub)`.

## Usage

Commit the built blob once into the repository that uses it (e.g. as
`tools/starlark.wasm`) and reference it from filter specs with the script
path as the first invocation argument:

```
:!tools/starlark=st/config.star[::st/]
```

The script path is looked up in the context-filtered tree; a missing
invocation argument or an absent script is a deterministic evaluation error
(trap → the filter degrades to `:empty`). Note the difference from the old
native filter, which treated a missing script as an empty script (nop).

## Building

From the repository root:

```
sh scripts/build-starlark-guest.sh
```

which is equivalent to, inside `josh-guest/`:

```
RUSTFLAGS='--cfg getrandom_backend="custom"' \
    cargo build --release --target wasm32-unknown-unknown -p josh-starlark-guest
```

The blob lands at
`josh-guest/target/wasm32-unknown-unknown/release/josh_starlark_guest.wasm`.
It is multi-MiB and deliberately **not** committed to this repository; test
fixtures copy it from the build output.

`josh-guest/.cargo/config.toml` sets the `getrandom_backend="custom"` cfg for
all wasm32 builds in this workspace, so a plain `cargo build`/`cargo check`
run from inside `josh-guest/` works too.

## Determinism

Filter evaluation must be a pure function of
`(module blob OID, invocation args, context-filtered tree OID)` — josh caches
results under exactly that key. Two measures keep the interpreter
deterministic:

- The wasm sandbox has no clock, environment, filesystem or randomness
  imports; the only host API is the `josh` import module.
- `getrandom` (a transitive dependency of the starlark crate, used for hash
  seeds) is routed to a stub that returns zeros
  (`src/getrandom_backend.rs`), so hash seeds are fixed. Using the `wasm_js`
  backend instead would add `__wbindgen_*` imports the josh host cannot link;
  the import section of the built blob must contain only `josh` entries.
