# RFC: Replace the Starlark filter with a WASM filter

## Summary

Replace the experimental `:!path[context]` Starlark filter (`josh-starlark` crate)
with a WebAssembly-based scripted filter. A commit's tree contains a wasm module;
josh instantiates it in a sandboxed, deterministic runtime, gives it read-only
access to the (context-filtered) tree, and the module returns the filter to apply.

The composition semantics of `:!` are kept, with one deliberate change: the
module blob is no longer forced into the output projection. The invocation
syntax gains an argument list (`:!path=arg,...[context]`) so a shared module
can be parameterized per invocation site.

## Background: how the Starlark filter works today

- Syntax `:!path[context]` parses to `Op::Starlark(path, subfilter)`
  (`josh-filter/src/op.rs:145`), gated behind `JOSH_EXPERIMENTAL_FEATURES`.
- `get_starlark()` (`josh-core/src/filter/mod.rs:458`) runs **per commit**:
  1. Loads the script blob from `path.star` *in the commit tree being filtered*.
  2. Applies the context `subfilter` to the tree; the result is what the script
     sees as `tree`.
  3. Evaluates the script; the script assigns a `Filter` to the global `filter`.
  4. Result is `compose([file(path.star), subfilter, script_filter])` — the
     script file itself is always part of the output.
  5. Any evaluation error silently degrades to `Op::Empty` (trace-logged).
- The script API is a host-defined mirror of the `Filter` builder
  (`josh-starlark/src/filter.rs`) plus read-only tree access
  (`josh-starlark/src/tree.rs`: `file`, `files`, `dirs`, `tree`).

Two properties of this design are load-bearing and must be preserved:

1. **The script lives in the tree it filters.** Old commits are filtered with
   the script they contained at the time; history stays self-consistent and
   reproducible with no out-of-band state.
2. **Evaluation must be a pure function of (script blob OID, filtered tree
   OID).** Filter results are cached persistently; any nondeterminism corrupts
   the cache and breaks round-trip guarantees.

## Motivation

Why replace Starlark with WASM:

1. **Sandboxing with hard limits.** Scripts come from repo content: in a
   josh-proxy deployment, anyone who can push can get code evaluated inside the
   proxy. Starlark restricts the language, but gives no hard instruction or
   memory budget out of the box. WASM engines provide fuel metering and memory
   caps as first-class primitives; a runaway or adversarial script becomes
   "evaluation failed → `Op::Empty`", not a stuck or OOM'd proxy.
2. **Language-agnostic authoring, without giving up hermeticity.** Starlark
   is deterministic and hermetic *by language design* — that is its purpose,
   and the current filter inherits it (josh's bindings are pure). But that
   guarantee is tied to the one language. Wasm enforces the same property at
   the instruction-set level: with zero WASI and only josh-defined imports, a
   module cannot observe time, randomness, environment, or the filesystem —
   regardless of what language it was compiled from. That is what makes it
   safe to accept filters written in Rust, Go (TinyGo), AssemblyScript, C —
   anything that targets wasm — without trusting each language runtime to
   have been designed like Starlark. Complex org-specific filter logic gets
   real tooling: types, tests, editors, code review of source.
3. **Performance headroom.** Evaluation is per-commit and can run over
   millions of commits. Validated/compiled modules are cached by blob OID;
   instantiation is microseconds. Starlark re-parses + interprets (with an AST
   cache at best).
4. **Implementation hygiene.** `josh-starlark` needs an `unsafe` raw-pointer
   `StarlarkTree` with hand-rolled `Send`/`Sync` impls to smuggle the repo
   reference through the interpreter (`josh-starlark/src/tree.rs:14-26`). A
   wasm host API threads context through the store naturally — no unsafe. It
   also drops the `starlark` + `allocative` dependency stack.

## Design

### Semantics

The composition shape of `:!` is kept, with two deliberate changes: the module
is named directly in the filter spec, and the invocation can pass arguments:

```
:!path=arg1,arg2[context]
```

- The module is the blob at `path.wasm` in the commit tree being filtered.
- The optional `=arg,...` list is passed to the module verbatim (see ABI).
  Arguments are part of the filter spec, not repo content; they lex like
  other flang string arguments (no escaping in v1, so no commas or brackets
  inside an arg).
- Context filter scopes what the module can read; it does not constrain the
  filter the module returns.
- Output is `compose([context, returned_filter])`. Unlike today, the entry
  blob is *not* forced into the output: the module appears in the projection
  only if the context filter includes it (rationale in the next section).
- Evaluation failure (trap, fuel/memory limit, bad output) → `Op::Empty`,
  trace-logged — same degradation as today.

`Op::Starlark` is renamed (e.g. `Op::Script` or `Op::Wasm`); parse, pretty,
persist, and the opt passes (`flatten`, `simplify`, `step`) update mechanically.
The feature is experimental, so no migration path for `.star` files is
required — the persist format tag can be reused or the version bumped.

### Module naming, arguments, and the projection

There is no indirection: `:!path` names the module blob directly, and a
shared module (an interpreter, or any reusable filter module) is committed
**once** and referenced from every filter expression that uses it. Repeating
the same path across many invocation sites is free — the module cache and
the evaluation memoization are keyed by content OIDs, so each distinct
(module, args, visible tree) combination is validated and evaluated once.

Per-site variation that would otherwise require a small entry file next to
each invocation site is carried by the `=arg,...` list instead. The
canonical example is an interpreter module taking its script path as an
argument:

```
:!tools/starlark=st/config.star[::st/]
```

```python
# st/config.star — an ordinary file, named freely; its path is explicit in
# the filter spec, so no naming convention or shebang line is needed
parts = [filter.subdir(d).prefix(d) for d in tree.dirs("")]
filter = compose(parts)
```

The interpreter reads the script through the ordinary tree imports, which
has two consequences, both intended:

- The script must be visible in the context-filtered tree (the invocation
  above works because `::st/` includes `st/config.star`). A missing or
  out-of-context script is a deterministic evaluation error.
- Whether filter configuration appears in the projection is the author's
  choice, expressed through the context filter: the script above lands in
  the output (editable in the projection, as today), while the multi-MiB
  interpreter blob stays out unless deliberately included. No entry-file
  special-casing is needed in the output composition.

Everything still resolves inside the commit tree being filtered, so the
self-contained-history property and evaluation purity are unaffected.

### Application semantics: commit level vs. tree level (inherited)

This behavior is inherited from the Starlark op unchanged — the wasm filter
slots into the same two invocation paths that exist today.

Evaluation is a pure function of the input tree in every position: the
module never sees commit metadata (unlike `:rev` / `:hook`, which are
resolved from commit identity). What varies with the op's position in the
filter expression is which tree it is given and what history machinery
wraps it:

- **Commit level** — `:!` as the top-level op or as a chain element
  (`apply_to_commit` applies chain elements one at a time to whole
  commits). The module is resolved from the commit's tree *and* separately
  from each parent's tree; the results go through the per-rev machinery, so
  when the evaluated filter differs between parent and child (module blob,
  visible context tree, or script changed) history splicing keeps the
  projection connected — the same treatment as `:workspace` and `:stored`.
- **Tree level** — `:!` nested inside a compose, an exclude/select
  subfilter, or `workspace.josh` content. It is evaluated on whatever tree
  flows to that point; no per-parent resolution, no splicing.

A consequence of chain semantics: in `:/sub:!tools/mod`, the module is
looked up in the already-`:/sub`-filtered tree. "The commit tree being
filtered" always means the input tree at the op's position in the pipeline.

Round-trip (push) support also follows the `:stored` pattern: the op has no
static inverse, so `unapply` handles it at commit-level positions via the
per-rev reverse path, and inside `workspace.josh` / stored content via
legalization (the op is substituted by its resolved filter against a
concrete tree before the invertibility check). Nested in a compose written
directly in the filter spec, it cannot be unapplied — true of `:stored`
today as well.

The memoization key covers both paths, since evaluation is tree-pure either
way; the commit-level path is simply the same tree function invoked on
commit and parent trees.

### Guest interface: handle-based filter builder

The guest constructs the filter through host functions that mirror the
`Filter` builder API — the same shape as the Starlark bindings today
(`josh-starlark/src/filter.rs`), translated to a wasm ABI.

This follows the rationale behind the original Starlark design, which
deliberately forbade filter-language strings in scripts:

- **Performance.** The builder methods construct the interned filter AST
  directly (`to_filter` on already-parsed ops). Returning flang text instead
  would put the pest parser on the per-evaluation path — for a filter that
  runs per commit, that is exactly the cost the builder API was introduced to
  avoid.
- **Correctness.** Builder arguments are plain strings; a textual filter
  would need flang quoting/escaping for paths with special characters, an
  entirely avoidable failure class.
- **Well-formedness.** A handle can only ever refer to a filter the host
  itself constructed; there is no intermediate representation that can be
  malformed.

Mechanically: the host keeps a per-evaluation handle table (`Vec<Filter>` in
the store data — `Filter` is a `Copy` wrapper around an OID, so this is
cheap). Guests hold opaque `u32` handles and combine them through imports.
The guest-side SDK wraps the handles in a typed `Filter` type so authors
never see raw `u32`s.

### ABI (v1)

Plain core-wasm ABI, no component model, no WASI. Strings are UTF-8
`(ptr, len)` in guest memory; filters are `u32` handles.

Guest exports:

| Export | Purpose |
|---|---|
| `memory` | linear memory |
| `josh_abi_version: () -> u32` | must return `1`; anything else → error |
| `josh_alloc: (len: u32) -> ptr` | host allocates guest buffers through this |
| `josh_run: () -> u32` | entry point; returns the handle of the resulting filter |

Host imports (module `josh`), in three groups:

**Filter constructors/combinators** — one import per `Filter` builder method,
mirroring the current Starlark method set (`nop`, `empty`, `subdir`, `prefix`,
`file`, `rename`, `pattern`, `chain`, `linear`, `workspace`, `stored`,
`author`, `committer`, `message`, `unsign`, `prune_trivial_merge`, `hook`,
`with_meta`, `insert`, `treeid`, `peel`, `is_nop`, plus `wasm(path, args, context)`
replacing `starlark(...)`). All take/return `u32` handles; string arguments
are `(ptr, len)` pairs. `compose` takes a `(ptr, len)` array of handles in
guest memory. An invalid handle traps (→ evaluation error).

**Tree access** — mirroring today's `Tree` methods, operating on the
context-filtered tree with no way to reach outside it:

| Import | Purpose |
|---|---|
| `tree_file(path) -> string` | blob content at path; empty if absent/binary — matches current semantics |
| `tree_files(path) -> string` | newline-joined child file paths |
| `tree_dirs(path) -> string` | newline-joined child directory paths |
| `tree_entry_oid(path) -> string` | hex OID of the object at path (enables content-addressed logic; cheap and pure) |

**Invocation context:**

| Import | Purpose |
|---|---|
| `invocation_args() -> string` | the newline-joined `=arg,...` list from the filter spec; empty if none |

Arguments are the only invocation-specific input: anything a module needs
beyond the visible tree (a script path, a mode flag) is spelled out at the
invocation site. There is no import for reading outside the context-filtered
tree — an interpreter module reads its script via `tree_file`, so script and
data alike are covered by the context filter and hence by the filtered tree
OID in the memoization key.

Host-to-guest data transfer goes through `josh_alloc`.

The import list is larger than a single-string interface, but each import is
a one-line delegation to an existing `Filter` method (exactly as in
`josh-starlark/src/filter.rs`), and ABI evolution is naturally backward
compatible: modules only link the imports they use, so adding builder methods
never breaks existing modules.

Newline-joined lists keep the tree-access encoding at "strings only" for v1;
git path components cannot contain `/`-escaping issues but *can* in principle
contain newlines — the host skips such entries (they are pathological in git
anyway). A length-prefixed encoding can replace this in a later ABI version
if needed.

The component model / WIT was considered and rejected for v1: it would pull
in much heavier runtime machinery for what is a flat list of thin delegating
functions. The `josh_abi_version` export leaves the door open.

### Runtime choice: `wasmi` first, engine behind a trait

Two realistic candidates:

- **wasmi** — pure-Rust interpreter. Small dependency, built-in deterministic
  fuel metering, no JIT (no executable-page story to defend in the proxy),
  trivially portable. Slower per instruction, but filter scripts are small and
  dominated by host calls (tree access).
- **wasmtime** — Cranelift JIT. Much faster execution, pooling allocator,
  epoch interruption. Heavy dependency; brings a compiler into josh-proxy.

Recommendation: **wasmi**, kept behind a small internal trait
(`instantiate + run with fuel/memory limits`) as cheap insurance. Raw
execution speed matters less than it first appears: the context filter gives
evaluations a low-entropy memoization key (see "Caching" and the Starlark
compatibility section), so modules run when their visible input changes, not
per commit. For a v1 experimental feature, the lighter dependency and
simpler security posture win.

### Determinism and resource limits

- **No WASI, no clocks, no randomness** — only the four `josh` imports.
- **Fuel limit** per evaluation (default on the order of 10^8 instructions,
  configurable via the same mechanism as other josh settings). Exhaustion →
  evaluation error → `Op::Empty`.
- **Memory limit** per instance (default e.g. 64 MiB) and a module size limit
  (e.g. 16 MiB blob).
- **Float determinism:** NaN bit patterns are the one nondeterminism corner in
  wasm. wasmi is deterministic here; if wasmtime is ever adopted, NaN
  canonicalization must be enabled. Worth a test either way.
- **No re-entrancy:** host imports cannot call back into filter application,
  so a module cannot recursively trigger unbounded filtering work.

### Caching

Two layers, both keyed by content:

1. **Module cache:** validated (and, if JIT, compiled) module keyed by the
   `.wasm` blob OID. In-memory LRU, analogous to the existing `WORKSPACES`
   map. Amortizes validation across the millions of commits that share one
   script version.
2. **Evaluation memoization:** the produced `Filter` keyed by
   `(module blob OID, invocation args, filtered tree OID)`. Since the ABI is
   pure, this is sound, and it means a history where the visible tree rarely
   changes evaluates the module only a handful of times. (Today's Starlark
   path re-evaluates per commit unconditionally.)

Downstream, the returned `Filter` participates in normal persistent filter
caching unchanged.

### Authoring experience

Binary blobs in git are the real cost of this design (a `.star` file is
reviewable; a `.wasm` blob is not). Mitigations:

- **`josh-filter-guest` SDK crate** (Rust, `no_std`-friendly): typed `Filter`
  wrapper over the handle imports, safe wrappers over the tree imports, the
  alloc/entry-point boilerplate behind a macro. A filter like today's doc
  example becomes:

  ```rust
  #[josh_filter]
  fn run(tree: &Tree) -> Filter {
      compose(tree.dirs("").map(|d| subdir(&d).prefix(&d)))
  }
  ```

- **Convention:** commit the source next to the blob (e.g.
  `st/config/src/…` + `st/config.wasm`) so review sees source and diff sees
  blob-changed; reproducible `wasm32-unknown-unknown` builds make the pair
  verifiable.
- **`.wat` support** (cheap, worth including): if `path.wasm` is a text blob
  that parses as WAT, assemble it. Small filters stay human-readable in the
  repo, closing most of the reviewability gap for simple cases.
- Optional later: `josh filter-build` helper subcommand that compiles and
  vendors a filter; explicitly out of scope for v1.

### Starlark compatibility via a wasm-hosted interpreter

Starlark support does not have to be dropped: a Starlark interpreter compiled
to wasm can run as an ordinary guest module. starlark-rust builds for
`wasm32-unknown-unknown` (it powers the browser playground), so a
`josh-starlark-guest` crate can wrap it with bindings that map the script's
`filter.*` methods onto the handle imports and `tree.*` onto the tree
imports — the same script API as today, executed inside the sandbox. The
module takes the script path as its argument, reads the source via
`tree_file`, and returns the handle of the filter the script constructed.
No Starlark-specific code path exists in the host.

Two deployment variants:

1. **Checked into the user's repo, once.** The repo commits the interpreter
   blob at a single location (e.g. `tools/starlark.wasm`); each invocation
   site is `:!tools/starlark=path/to/script.star[context]`. One ~2–4 MiB
   object per repo, one entry in the module cache, and adding a new scripted
   filter site is just adding a `.star` file and referencing it. Josh itself
   stays Starlark-free.
2. **Embedded in josh.** Josh vendors the blob (built reproducibly from the
   workspace crate); `:!path` where `path.star` exists (and `path.wasm` does
   not) runs the embedded interpreter with that script. Fully backward
   compatible with existing `.star` filters; the starlark dependency moves
   out of the native josh-proxy binary into a wasm32 build artifact.

Either variant has two side benefits: Starlark scripts inherit the fuel and
memory limits (which they lack today), and the interpreter is an existence
proof that the ABI is expressive enough to host a full scripting language.

Double interpretation under wasmi — a Starlark interpreter running inside a
wasm interpreter — is plausibly one to two orders of magnitude slower than
today's native evaluation, but this is acceptable by design rather than by
accident: the context filter exists precisely to give the evaluation a
low-entropy memoization key. The script sees only the context-filtered tree,
so the `(module OID, args, filtered tree OID)` key changes only when
the *visible* tree changes — with a well-scoped context filter (e.g. just the
config directory), the interpreter runs once per config change, not per
commit. Authors who need a genuinely fast computed filter on a
high-entropy input write a native wasm module directly; that is the escape
hatch, not a faster engine.

Determinism of the interpreter itself needs verification once, not per
script: wasm modules have no entropy imports, so hash seeds are fixed, but
iteration-order dependence in starlark-rust internals should be checked
before the blob is blessed.

### What gets deleted

- The `josh-starlark` crate (~1000 lines, incl. the unsafe `StarlarkTree`).
- The `starlark` / `allocative` dependency stack.
- The Starlark section of `docs/src/reference/experimental.md`, replaced by
  the wasm filter reference and ABI documentation.

If the wasm-hosted interpreter (previous section) is adopted, the deletion
still holds for the host: the starlark dependency survives only inside the
`josh-starlark-guest` wasm32 artifact, and the unsafe `StarlarkTree` plumbing
is gone regardless — tree access goes through the host imports.

## Alternatives considered

- **Keep Starlark, add limits.** starlark-rust can be instrumented, but
  limits stay best-effort, the unsafe repo-pointer plumbing stays, and the
  single-language ceiling stays. Doesn't address motivations 2–4.
- **Guest returns a flang string, host parses it.** Collapses the ABI to
  four tree imports plus one string return, and reuses the filter language as
  the interchange format. Rejected for the same reasons the Starlark API
  forbade filter strings: it puts the pest parser on the per-evaluation path
  (the builder constructs the interned AST directly), and it reintroduces
  flang quoting/escaping as a failure class for paths with special
  characters. The larger import list is thin delegation code, not logic.
- **In-tree pointer files / shebang indirection.** An earlier draft resolved
  `:!path` through small pointer blobs (a `path.star` whose first line is
  `#!tools/starlark.wasm`) so one shared module could serve many per-site
  entry files. Rejected: naming the module directly in the filter spec and
  passing per-site input via `=args` covers the same cases with no
  resolution rules, no pointer-file conventions, and no entry-file
  special-casing in the output composition. Referencing the same module path
  from many filter expressions is free because everything is keyed by blob
  OID.
- **Component model (WIT).** Right long-term shape, wrong cost for a flat
  delegating interface in an experimental feature. Revisit at ABI v2 if the
  interface grows.
- **Out-of-repo module registry (fetch by hash).** Breaks the self-contained
  history property (1) above; rejected.

## Open questions

1. **Silent `Op::Empty` on failure** is inherited from Starlark. With hard
   fuel limits, a legitimate script hitting the ceiling silently produces an
   empty tree. Should evaluation failure become a filter *error* (visible to
   the user) instead? Leaning yes, but it is a semantic change worth deciding
   deliberately; it equally affects the existing Starlark behavior.
2. **Op name and syntax.** Keep `:!path` or introduce a distinct sigil while
   both exist during development? (Since the feature is experimental, outright
   replacement in one release is the simple answer.)
3. **Recursion depth.** The `wasm(path, args, context)` builder method lets
   a module reference another wasm filter (as `filter.starlark(...)` does
   today); evaluation depth needs an explicit cap.
4. **Default fuel/memory numbers** — need measurement against realistic
   filters before fixing defaults.
5. **Where the SDK crate lives** — in-tree (`josh-filter-guest`, published) vs.
   separate repo. In-tree keeps ABI and SDK in lockstep; recommended.
6. **Starlark compatibility variant** — checked-in interpreter blob (josh
   stays Starlark-free) vs. embedded in josh (full backward compatibility for
   existing `.star` filters).
7. **Argument syntax limits.** `=arg,...` inherits flang lexing, so args
   cannot contain commas, brackets, or whitespace in v1. Whether an escaping
   scheme is ever needed (paths with unusual characters) can be deferred.
8. **Invertibility of the returned filter.** `:stored` content is
   `invert()`-checked at load time and degrades to empty when not
   invertible, so a stored filter is invertible by construction. A script's
   returned filter is unchecked: a module can return a non-invertible
   filter (e.g. `:prune`), which works fine for read-only projections but
   only fails at push time — or, nested inside a `workspace.josh`, silently
   empties the whole workspace filter via the legalization invert check.
   Should the host check the returned filter: always (deterministic
   evaluation errors, but forbids legitimate non-invertible uses), never
   (status quo), or only in contexts that require invertibility (keep the
   check where it lives today, in stored/workspace loading, and document
   the constraint for pushable paths)? Leaning towards the last option.
