# RFC: Views

## Summary

Introduce *views*: named, versioned filter objects stored as git refs. A view is any ref whose
commit tree contains a `view.josh` file. Views supersede the current workspace mechanism: the
workspace behavior (compose a root directory with its in-tree stored filter) becomes the default
template instantiated on the view mechanism. The existing workspace feature and views coexist for
a migration period; deprecating and eventually removing `Op::Workspace` in favor of the nearly
identical `Op::Stored` is a future step, taken only after users have had time to migrate. Views
give a versioned, mutable home for per-workspace policy (meta options, compat flags) and for
migration points (`:rev`), without rewriting source history.

## Motivation

The current workspace design entangles the filter definition with the history it filters:
`workspace.josh` lives inside the thing it defines. This entanglement is the root cause of several
of josh's hairiest mechanisms and limitations:

- **No home for policy.** Meta options (`:~(...)` history flags, gpgsig handling, future compat
  flags such as a pattern-semantics flag) have no per-workspace, versioned, mutable place to live.
  Old `workspace.josh` files frozen in history cannot be updated, so compat flags end up as
  deployment-global configuration.
- **No first-class migration points.** When a workspace's definition changes structurally (root
  moves, semantics flips), there is no way to record "before commit X, use the old definition"
  except implicitly through filter-change splicing.
- **Per-branch identity.** Today "the workspace `a`" is not one thing — it is whatever
  `a/workspace.josh` says on the branch being fetched. The same workspace URL can mean structurally
  different filters on different branches. For a named entity used in CI configs and remote URLs,
  per-branch identity is the wrong coupling.
- **Redundant ops.** `Op::Workspace` and `Op::Stored` are one operation wearing two syntaxes; the
  docs already explain workspaces by the equation "`:/a` combined with applying a stored filter".

## Design

### A view is a ref containing `view.josh`

A view is **any ref** whose commit tree contains a `view.josh` file — by default at the tree
root, or at a location selected by a *locator filter* (see below). There is no mandatory
namespace; `refs/josh/views/<name>` is only the default location in the name-resolution search
path (see URL syntax below).

Because views can live in ordinary branches, they inherit the full forge feature set on GitHub,
GitLab, etc.:

- View changes are pull requests: review, CODEOWNERS, branch protection, required CI, audit trail.
- Views replicate, back up, and mirror like any branch — no special refspec configuration.
- Access control for "who may change this view" is "who may push that branch".

The recommended shape is an **orphan branch** containing only `view.josh`, so view history does not
tangle with source history. The `josh view create` command sets this up; nobody needs to know the
git incantation.

#### Locator filters: views inside existing branches

Some users will want to store views in an existing branch (typically the main branch), but not at
its root. To enable this, a view reference may carry a **locator filter**: a filter applied to the
ref's tree before `view.josh` is looked up at the result's root. A view reference is therefore in
general a pair (ref, locator), where the locator defaults to `:/` (root). Locating a filter
definition *via* a filter is the same move the workspace template already makes with stored
filters — the language locates its own definitions; no separate path convention is needed.

Example: with ref `refs/heads/main` and locator `:/views/foo`, the view is defined by
`views/foo/view.josh` on `main`.

Properties and constraints:

- **Locators are restricted to path-selecting filters** (subdirectory chains, essentially): no
  stored filters, no views, nothing that itself requires resolution. This keeps view resolution a
  single non-recursive step and structurally preserves the no-cycles guarantee.
- **Globality survives.** Resolution still reads one ref (the unfiltered tip of `main`) regardless
  of which branch is being fetched. What changes is that view history is interleaved with the
  branch's history (`git log -- views/foo/` instead of a dedicated ref log) — an acceptable trade
  that is the user's choice to make.
- **Workspace ergonomics are recovered.** A view stored in `main` gets code review with the code,
  and if the view's own definition directory is included in its filter output, round-trip pushes
  can edit the view — precisely the current `workspace.josh` behavior. Views-in-main-branch is
  therefore the natural migration target for existing workspace users.

Views live **upstream** on the forge, as ordinary repo content — not as proxy-local state. When
resolving a view, josh always reads the view ref from the *unfiltered* upstream's current state
(views are themselves subject to filtering when fetched through josh, so resolution must not go
through the filtered lens).

This also enables **review-gated filter allow-listing**: a deployment can be configured to only
serve views defined in protected branches matching some pattern. Which filtered projections of the
monorepo exist then becomes a reviewed, access-controlled artifact — a capability the current
design cannot express.

### Storage: `view.josh` text, not the serialized filter representation

The ref's commit contains a `view.josh` file holding the filter in flang syntax. It does **not**
use the serialized filter-tree representation (`as_tree`/`from_tree` in
`josh-filter/src/persist.rs`); that remains an internal cache/persistence format.

Rationale:

- **Consistency.** Everywhere else in josh, the source of truth for a filter is flang text stored
  in a git tree (`workspace.josh`, stored filters, `compose.josh`). The parse path, error handling
  for invalid filter text, and legalization against a tree already exist and are battle-tested. A
  binary representation would make view refs the only place filters persist in non-text form, with
  ambiguity about which format is canonical.
- **Comments survive.** This matters most for views: a `:rev(...)` migration point wants a comment
  next to it explaining *why* (e.g. "2026-07: root moved from a/ to b/"). The serialized form would
  normalize that away.
- **Legible history.** `git log -p <view-ref>` shows meaningful text diffs of the view's evolution.
- **Free escape hatch.** Power users can inspect or edit a view with nothing but git. A manually
  pushed invalid `view.josh` fails exactly like an invalid `workspace.josh` does today. The
  `josh view` command validates on write, so the normal path never hits parse errors.

Nothing is lost for caching: filter identity attaches to the parsed, resolved, interned filter, not
to the bytes. Two texts differing only in whitespace or comments resolve to the same interned
filter and share cache — the correct equivalence.

Since flang already supports `:~(...)` meta options, `#` comments, and newline composition, the
entire view — body, policy flags, migration points, commentary — fits in the one `view.josh` file.
The commit wrapper exists purely to give it history.

### A view is not a filter

A view *denotes* a filter; it is not an op in the filter language. Two structural reasons:

1. **Referential transparency.** A filter is a pure function of its input — `:/sub` means the
   same thing forever, which is what makes interning, caching, and the optimizer sound. A view
   resolves against a mutable ref. Making view lookup a first-class op would put external mutable
   state inside the algebra. (`LazyRef` in `:squash` flirts with this, but those are resolved once
   at invocation and frozen.)
2. **Policy placement.** A view carries evaluation pragmas — `:~(...)` history flags, migration
   points — that only make sense governing a whole filtering run. An op can appear nested anywhere
   in a composition; "what does `history=linear` mean three levels deep inside a compose" is a
   question we would be forced to answer for no benefit.

The resulting vocabulary, where every word carries weight:

- **filter** — an anonymous pure expression (a tree/history transformation).
- **view** — a named, versioned binding whose value is a filter plus pragmas, applied to a repo.
- **workspace** — the default view template: writable dev-checkout flavor (see below).

A view reference is therefore its own URL element — a peer of the `@headref` slot — constrained to
leading position, not an element of the filter spec. Ordinary filter chaining *after* the view
reference is allowed and well-defined: resolve the view to filter F (with its pragmas), then chain
the remaining ops on top of F's output.

Consequence, accepted deliberately: views cannot reference other views (no view op means no
`:=bar` inside a `view.josh`). For v1 this structurally rules out cross-ref resolution cycles. It
is a one-way door that can be opened later (promote to an op with cycle detection) but never
closed.

### The workspace template and filter/code co-evolution

The default view created by `josh view create --workspace <root>` has a body equivalent to the
current workspace semantics: select the workspace root and compose it with the stored filter
`<root>/workspace.josh` read from the tree being filtered.

The key property this preserves: **the filter body still co-evolves atomically with code.** The
view ref points at an in-tree stored filter, so day-to-day filter edits are ordinary source
commits — reviewed with the code they belong to, traveling with every clone, and editable from the
filtered checkout via push exactly as today. The view ref itself only changes at rare,
operator-level events.

This gives a clean division of labor:

- **In the tree** (via the stored filter): the filter proper — the thing that should change with
  the code, be reviewed with the code, and travel with every clone.
- **In the ref**: the things that never belonged in the tree — the view's identity, meta options
  and compat flags, and `:rev` migration points marking where the definition structurally changed.

Two evolution channels coexist: fine-grained changes flow through the in-tree stored filter and
still use the existing machinery (splicing, legalization against the tree at each commit);
coarse-grained changes (root moves, semantics flips) flow through view-ref commits and `:rev`
migration points. Ref updates are explicit via the `josh view` command; josh does not write to
view refs automatically (automatic migration-point recording on push may come later, once the
semantics are proven).

### Views apply to all branches equally

A view ref is repo-global: its identity and policy are uniform across all branches, unlike today's
per-branch `workspace.josh` semantics. The body can still vary per commit via the stored-filter
delegation — branches can evolve *what is in* the workspace; they cannot disagree about what the
view fundamentally *is* (its root, compat flags, migration points).

Migration points compose correctly with branching by construction: ancestry-based `:rev` matching
(`<=sha`) does not care about branch names, so a migration point applies to exactly the commits
descended from (or preceding) it, uniformly across every branch containing them.

Branch-local *policy* experiments are replaced by minting a scratch view pointing at the same
root — policy experiments get their own name instead of hiding inside a branch.

### Coexistence and eventual `Op::Workspace` deprecation

The existing workspace feature (`:workspace=`, `Op::Workspace`) remains fully supported while
views are introduced. The two features coexist for a migration period so users can move at their
own pace; nothing breaks on day one.

Deprecating and removing `Op::Workspace` is a **future step**, taken only after migration tooling
exists and adoption has proven the view mechanism. The eventual unification is semantically safe:
the equation `:workspace=a` ≡ `:/a` + stored filter from `a/workspace.josh` (composed with the
root) is already how the docs explain workspaces, so replacing the op with `Op::Stored` is a
hash-preserving refactor. `:workspace=` may survive beyond that as parse-level sugar during a
deprecation period.

### URL syntax

A view reference is spelled `:=<name>`, leading-position-only within the spec slot:

```
repo.git:=foo.git                    view foo
repo.git:=foo:/sub.git               subdirectory of view foo (chaining after resolution)
repo.git@release-1.0:=foo.git        view foo applied at base rev release-1.0
repo.git:=main[:/views/foo].git      view at views/foo in ref main (explicit locator)
```

Rationale:

- `:=` is the definition/binding operator (Pascal, Go, Python's walrus), and a view *is* a named
  binding: `repo.git:=foo.git` reads as "the spec is defined by foo". The symbol points at the
  semantics (a definition), not the storage substrate (a ref).
- The URL-safe symbol palette (RFC 3986 pchar: `: @ ! $ & ' ( ) * + , ; = - . _ ~`) is nearly
  exhausted by flang, and `:=` is nearly free in the sigil budget: `=` is structurally locked into
  the `cmd=arg` position and can never lead a filter, so the grammar spot is unambiguously
  unclaimed. This preserves `@` for a possible future ref-shaped use *inside* the language (e.g.
  resolve-at-invocation ref syntax in `:rev(...)`), where it would be irreplaceable.
- It avoids a double-`@` URL: `repo.git@release-1.0:=foo.git` keeps each symbol to one job,
  whereas an `@`-based spelling would put two `@`s with different meanings a few characters apart.
- The known cost: visual adjacency to argument-`=` — `:=foo` could scan as a malformed `:cmd=arg`.
  Mitigated by the leading-position-only rule; `:` immediately followed by `=` occurs nowhere else
  in the language.
- The string `:=foo` already lands in the proxy's existing `filter_spec` capture (`[:!].*`), so the
  proxy URL regex is unchanged; the "leading position, at most one" rule lives in the flang-side
  grammar. That restrictedness encodes "a view is not a filter" in the syntax itself.
- In `repo.git@main:=foo.git`, `@main` picks the base rev being filtered; the view ref always
  resolves from the unfiltered upstream's current state, never from the base rev.

The explicit locator form `:=<ref>[<locator>]` puts the locator filter in `[...]` after the ref
name, consistent with how `:exclude[..]` and `:invert[..]` take filter arguments; absent brackets
mean `view.josh` at the ref's root. The bracket delimiter is what disambiguates the locator
(applied to the ref's tree to *find* the view) from the post-view chain (applied to the view's
*output*): in `:=main[:/views/foo]:/sub`, `:/views/foo` locates the view and `:/sub` refines its
result. (`[`/`]` are RFC 3986 gen-delims and technically require encoding in URLs, but this cost
already applies to compose filters in URLs today, and git and curl pass them through in practice;
the bracketed form should be rare in URLs anyway — see search paths below. A DWIM alternative,
`:=main/views/foo` with longest-existing-ref-prefix splitting, was rejected: it makes resolution
depend on which refs happen to exist, silently changing meaning if a ref named `main/views` ever
appears.)

Short names resolve via a configurable DWIM search path whose entries generalize to
(ref, locator-template) pairs — default: `refs/josh/views/<name>`, then `refs/heads/views/<name>`;
a deployment can add e.g. `refs/heads/main` + `:/views/<name>` so that `repo.git:=foo.git` finds
`views/foo/view.josh` on `main` with no URL syntax at all. "Where views live" is thereby a
search-path question, which is where it belongs. Fully-qualified `refs/...` names are accepted
verbatim; ambiguity is an error, never a silent pick.

Flang-free URLs for monorepo consumers are a deployment-layer concern, not core syntax: a proxy
alias map (`frontend.git` → `monorepo.git:=frontend`) provides vanity naming, deprecation
redirects, and per-team URL policy. Path-based view addressing in core (`repo.git/foo.git`) is not
workable — it is ambiguous against nested repo paths on GitLab-style forges, which is why the spec
slot is `:`-delimited in the first place.

### On the name

"View" revives josh's original terminology (pre-2019 josh had `ViewMap` / `apply_view_cached`
before the rename to "filter") — but at the right layer this time. The database analogy is exact:
a database view is a named stored query over a base table; a *materialized* view caches its
results (josh's caching model); an *updatable* view is one you can write through, with defined
rules translating writes back (apply/unapply, the push round-trip). "Workspace" does not die — it
names the default template, the writable dev-checkout flavor. Other flavors (published read-only
projections, per-commit-policy views replacing `:hook` use cases, sync configurations) get their
own names on the same substrate.

## Migration

Importing an existing workspace is mechanical and hash-preserving: mint a view whose `view.josh`
is the workspace root composed with the in-tree stored filter — semantics identical by
construction, so filtered trees are byte-identical and no history diverges.

## Implementation order

1. View resolution: ref lookup, DWIM search path, `view.josh` parsing, `:=name` URL element.
2. `josh view` command surface: create (orphan branch setup, workspace template), modify, list.
3. Workspace import tooling. Workspaces and views coexist from here on.
4. *(future, after the migration period)* Deprecate `Op::Workspace` and unify it into
   `Op::Stored` — a hash-preserving refactor that shrinks the op algebra.

## Open questions

- Exact DWIM search path defaults and its deployment configuration syntax.
- Whether josh ever auto-writes migration points to view refs on push (e.g. when a push changes
  the stored filter's location), or ref updates stay explicit-only. Leaning explicit-only first.
- Duration of the coexistence period and the deprecation timeline for `Op::Workspace` /
  `:workspace=` syntax.

## Future work (explicitly out of scope for v1)

- Deprecation and removal of `Op::Workspace` (unification into `Op::Stored`), after the
  coexistence period.
- Views referencing other views (requires promoting the view reference to an op with cycle
  detection).
- Automatic migration-point recording.
- Proxy-level view alias maps for flang-free consumer URLs.
