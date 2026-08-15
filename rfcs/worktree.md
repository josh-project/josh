# RFC: Worktree-centric CLI

## Summary

Today josh-cli mirrors git's porcelain: `josh clone <url> <filter> <dir>` creates a working
copy checked out at the filtered projection, and `josh fetch` / `pull` / `push` are
filter-aware drop-in replacements for their git counterparts, with the filter fixed at clone
time and stored in the remote's configuration. This RFC replaces that model with the
bare-repo-plus-worktrees pattern: `josh init` creates a shared store without a checkout,
`josh remote add` attaches upstreams to it, and `josh worktree add` creates filtered worktrees.
Filters thereby attach to worktrees instead of to a clone; many worktrees with different
filters share one object store, one cache, and one network fetch. A worktree sees only the
refs of its own projection: its remotes are all served by the store through git namespaces, so
everything inside a worktree — including `git fetch` and `git push` — is plain git, while josh
moves history between the store and the upstreams. Remotes, too, can sit at a projection level
(rust-lang/miri holds miri-level history of rust-lang/rust), so a change can flow to both the
projected repo and the wider repo it projects from.

## Motivation

### The drop-in model does not hold together

The current model — `josh clone` / `josh fetch` / `josh pull` / `josh push` as filter-aware
replacements for the corresponding git commands — has structural problems that no amount of
polish fixes:

- **Chasing git's surface.** A drop-in replacement implicitly promises parity with git's UX:
  every flag of `git clone`, `git fetch`, `git pull` is a feature request. The current
  `PullArgs` already re-plumbs `--rebase` and `--autostash` through to git one flag at a time.
  This treadmill has no end, and everything not yet re-plumbed is a silent gap.
- **Half-working git commands.** To make plain `git fetch origin` do something at all, the
  current setup points `origin` at the repository itself with a `GIT_NAMESPACE` upload-pack
  (`handle_remote_add_repo` in `josh-cli/src/bin/josh.rs`). The result is a remote that
  impersonates the upstream while serving whatever josh last filtered — plausible-looking but
  stale. A git command that silently means something else is worse than one that fails: users
  cannot tell which commands are safe. (The namespace mechanism itself is sound; the design
  below reuses it at an honest boundary.)
- **One filter per clone.** The filter is fixed at clone time and stored in the remote config
  (`.git/josh/remotes/<name>.josh`). Working on a different scope means a second full clone with
  its own cache, its own fetches, and its own filtering work. Wanting two filters against the
  same upstream means two remotes in one repo or two repos — both awkward, neither intended.
- **Filter attached to the wrong thing.** A remote is *where* code lives; a filter is *which
  part* you are working on. Binding them means the question "what am I looking at" is answered
  by remote configuration rather than by the checkout in front of you. There *is* a legitimate
  per-remote fact in the vicinity — some remotes' history genuinely lives at a projection level
  (rust-lang/miri relative to rust-lang/rust) — but the current config cannot express that
  separately from the checkout choice; the design below splits the two.

### The world moved to bare + worktrees

Meanwhile, the pattern of cloning once (bare) and using `git worktree` for every checkout has
become the standard workflow for parallel work — driven hard by coding agents, where each agent
gets its own worktree, but equally useful for humans juggling tasks. The pattern won because
worktrees are cheap (shared objects, isolated `HEAD`/index), consistent (one fetch updates all
checkouts), and disposable.

Josh fits this pattern unreasonably well — better than it fits the drop-in model:

- **A filter is exactly a worktree-shaped concept.** "This checkout is the frontend projection"
  is per-checkout state, which is precisely what worktrees isolate. Different agents (or tasks)
  get different projections of the same repo, side by side.
- **Shared cost.** Unfiltered history is fetched once for all worktrees. Filtering work is done
  once per filter and cached in the shared sled cache — which already lives in the common git
  dir for exactly this reason. N agents do not multiply network or filtering cost.
- **Scoped agents.** A filtered worktree is a physical scope boundary: the agent cannot read or
  touch files outside its projection, and its pushes go through unapply, which by construction
  only writes back into the filter's image. Filters become guardrails, not just conveniences.
- **A clean division of labor.** Everything local is plain git and *actually* works, because it
  operates on real, local, filtered history — no pretend upstreams, no parity chasing. Josh
  appears only at the seams: creating projections and syncing with upstream.

## Design

### `josh init` creates a store, `josh remote add` attaches upstreams

There is no `josh clone`. Cloning means "make me a copy of *that* repository", which quietly
reintroduces a privileged remote — the thing the levels model below deliberately does not have.
The two primitives are instead:

```
josh init [<dir>]
josh remote add <name> <url> [<filter>]
```

`josh init` creates `<dir>` containing a checkout-less repository (the *shared repo*): the josh
cache and no history yet. `josh remote add` records a remote (URL, forge config) and fetches
its history. Since there is no clone, there is also no implicit `origin`: remotes have explicit
names, as befits peers. The filter argument of `remote add` does not configure any checkout
(that is the worktree's job); it declares the *level* the remote's history lives at — see
"Levels" below.

This directory — shared repo plus the worktrees created inside it by default — is called the
**store**: the place all history is stored, and that worktrees check out from and push back
into. The word extends git's own vocabulary (the object store) rather than importing anyone
else's. Names rejected: "workspace" is taken by `workspace.josh` and the views RFC; "container"
collides with the podman containers `josh compose` runs tests in; "depot" is Perforce
vocabulary.

```
myrepo/
  .git/          shared repo: refs and objects of all remotes, josh cache
  frontend/      worktree, filter :/frontend
  backend/       worktree, filter :/backend
  full/          worktree, unfiltered (:/)
```

The `GIT_NAMESPACE` mechanism of the current implementation survives in a changed, honest
role: the store serves each worktree through a namespace per level — see "Worktree isolation"
below. What is removed is the pretense that the namespace-served remote is the upstream.

### `josh worktree add` creates a filtered checkout

```
josh worktree add <path> [<filter>] [-b <branch>] [--ref <ref>]
```

filters the requested ref through `<filter>`, creates a worktree at `<path>`, and checks out a
branch of the filtered history. Omitting the filter (or passing `:/`) gives an ordinary
unfiltered worktree — the full-monorepo checkout is just another projection, not a special case.

A josh worktree is technically a *linked repository*, not a `git worktree` — invisible in daily
use, but load-bearing for ref isolation (see "Worktree isolation" below). The filter is
recorded as `josh.filter` in the worktree's own git config, and `josh worktree list` / `remove`
manage the worktree registry in the store.

Once the views RFC lands, the filter argument can also be a view reference (`:=frontend`), and
`josh worktree add` becomes the primary consumer of views: `josh worktree add ../frontend
:=frontend`.

### Worktree isolation: levels are namespaces

A worktree must see only the refs of its own level. Refs of other levels are not merely noise
in `git branch -a`, `git log --all`, and shell completion — they are traps: another level's
branch names an unrelated history, and checking it out would produce garbage. This rules out
implementing josh worktrees as `git worktree`s, which share the entire ref store by design (the
per-worktree `refs/worktree/*` space cannot hold branches, and `GIT_NAMESPACE` has no effect on
local commands — it exists only at the protocol layer, in `upload-pack` and `receive-pack`).

The protocol layer is therefore exactly where josh puts the boundary. In the store, every level
is a git namespace: all refs belonging to a level live under `refs/namespaces/<level>/…`,
subdivided by the remote they correspond to. A worktree is a linked repository — own refs, own
config, objects shared with the store via alternates — whose git remotes are all served by the
store: a worktree at the miri level gets remotes `rust` and `miri`, each backed by
`GIT_NAMESPACE=<level>/<remote> git upload-pack` / `receive-pack` against the store. The
worktree's remotes mirror, by name, the upstreams visible at its level.

Everything follows from this arrangement:

- **Plain `git fetch` and `git push` are correct.** They exchange refs with the store — always
  local, never touching the network. "The store's current state of this level" is precisely
  what they should exchange; a `git push` parks a branch in the store's namespace, and nothing
  leaves the machine until a josh command relays it.
- **Upstreams render naturally.** Level refs arrive as ordinary remote-tracking refs:
  `git status`, `@{upstream}`, and `git rebase` show `rust/main` and `miri/master`, not
  `refs/josh/filtered/<hash>/…`.
- **Branches are per-worktree.** Each worktree owns its `refs/heads/*`: two agents can both
  have a `fix` branch without colliding, and a branch cannot accidentally travel to a worktree
  at a different level. The mismatch detection a shared ref store would need has no counterpart
  here.
- **The old trick is redeemed.** The current implementation uses the same namespace mechanism
  but points it at a remote impersonating the upstream. Here the store is openly the worktree's
  counterparty, and staleness is well-defined: git commands reflect the store; josh commands
  refresh it.

Two worktrees at the same level attach to the same namespace and thus share its refs;
refiltering happens once per level, not once per worktree. Transfers between worktree and store
are local and cheap, though avoiding duplicate object storage needs care (alternates make
objects reachable, but fetch negotiation does not know that) — tracked in the open questions.

### Levels: histories related by filters, with no canonical history

The motivating case is rust-lang/rust and miri. Miri is simultaneously a directory in the
monorepo (`src/tools/miri` — a view of rust) and a repository of its own (rust-lang/miri) with
its own PRs, issues, and CI, kept in correspondence by josh-based subtree sync. Someone working
on miri wants both as upstreams for the same change.

An obvious way to model this would be to designate the first remote's history as *canonical*
and express every other remote relative to it. That privilege is an artifact of setup order,
not of the world: rust and miri are peers — two histories related by a filter, both real, both
with their own branches and their own life. Which one a store happened to add first carries
no meaning, and the model must not change shape when a miri developer starts from miri and adds
rust second. There is no canonical history.

Instead, a **level** is a place a history sits, and levels are related *pairwise* by filters.
Remotes and worktrees each sit at a level; a declaration creates a filter edge between two
levels:

```
josh init rust
josh remote add rust https://github.com/rust-lang/rust
josh remote add miri https://github.com/rust-lang/miri :=miri
josh worktree add miri :=miri
```

The second `remote add` states "miri's history is rust's history through `:=miri`"; the
`worktree add` places the worktree at that same level. The base of a declaration defaults to
the first remote purely as UX convenience, with no semantic weight. The store thereby holds a
small graph of histories connected by filter edges. Each *edge* is directed —
apply goes toward the projection, unapply toward the wider history — but the graph as a whole
has no root. When the wider repo arrives second (add miri first, rust later), the new edge
simply points the other way: "miri is rust through `:=miri`"; the CLI needs a spelling for both
directions.

Fetch and push between a worktree and a remote translate along the path between their levels:

- **Same level** (miri worktree ↔ miri remote): plain git fetch and push — no filtering, no
  unapply. The fetched native history shares ancestry with the projection of rust through past
  sync points, so git-level merge, rebase, and comparison between the two just work.
- **One edge apart** (miri worktree ↔ rust remote): today's josh semantics — fetch applies the
  edge's filter, push unapplies along it.
- **Longer paths** (e.g. between two sibling projections of the same repo): each edge traversal
  is one apply or unapply. The model permits it; v1 supports only paths of length zero or one,
  since no known workflow needs more.

The payoff in the miri worktree: branches can track both the filtered monorepo (rust through
the miri view) and native `miri/master`. The same branch can be pushed to miri directly — plain
push, native review and CI — and later into rust via unapply. This is the maintainer workflow
that the rust-lang josh-sync scripts implement externally today, made native to the CLI.

Once the views RFC lands, a level is naturally *named* by a view — and in this scenario it is a
*reverse view* (see the views RFC): the miri remote itself hosts a `view.josh` declaring "I am
`:/src/tools/miri` of rust-lang/rust", since rust does not know josh exists and will never host
the definition. The edge declaration collapses into a view reference as written above, and
becomes self-describing — `josh remote add` pointed at miri can read the relationship from miri
instead of requiring it on the command line.

In storage terms, levels and namespaces coincide: refs fetched from a remote land in its
level's namespace as-is, refs filtered along an edge land in the target level's namespace, and
a worktree sees exactly its level's namespace content as its remotes — same-level refs with no
filtering pass in between, cross-level refs refiltered once per edge regardless of how many
worktrees consume them.

### Sync commands: git moves history locally, josh moves it across boundaries

The worktree↔store boundary belongs to git; the store↔world and level↔level boundaries belong
to josh:

- **Plain `git fetch` / `git push`** in a worktree exchange refs with the store's namespaces —
  always local, always safe, never on the network.
- **`josh fetch`** (anywhere inside the store) fetches each upstream into its level's
  namespace, then refilters along every declared filter edge, updating the other levels'
  namespaces. All worktrees see the update on their next `git fetch`, which `josh fetch` runs
  for them by default.
- **`josh push`** relays a branch parked in a level's namespace out to the actual upstream:
  plainly for a same-level remote, unapplying along the edge for a remote one edge toward the
  wider history — the existing push machinery, with the filter sourced from the edge instead of
  the remote config.
- **`josh pull`** is `josh fetch` plus integrating the current branch (delegating the
  merge/rebase to git, as today). **`josh changes …`** (stacked changes) builds on push,
  unchanged in semantics.

Everything else — committing, branching, rebasing, diffing, bisecting — is plain git in the
worktree, with no josh wrapper and no caveats, because it only ever touches real local filtered
history.

### Comparison: why not sparse checkout

Git's own answer to scoped checkouts, `git sparse-checkout`, covers a different need: it narrows
the working tree but keeps full history, full paths, and full-repo semantics for every command.
It offers no path remapping, no filtered history (log/blame still see the whole repo), and no
notion of pushing a projection back. Filtered worktrees provide true scoped history with
round-trip semantics; the two can even coexist (a sparse checkout of a filtered projection).

## Migration

The `clone` verb is removed immediately; there is no deprecation sugar. The setup path is
`josh init` + `josh remote add` + `josh worktree add`, and keeping a clone shorthand around
would only prolong the mental model this RFC retires. Existing old-style clones (detected by
the filter-bearing remote config) can be converted in place by a `josh migrate` helper that
turns them into a store with a single worktree at the old filter's level.

## Implementation order

1. `josh init` and the store layout with level namespaces; `josh remote add` with level
   declaration.
2. `josh worktree add` / `list` / `remove` as linked repositories attached to store namespaces
   via namespace-scoped `upload-pack` / `receive-pack`.
3. `josh fetch` (fetch upstreams, refilter along edges); `josh push` / `pull` / `changes`
   relaying between levels and upstreams.
4. Migration helper and docs.

## Open questions

- Exact store layout: bare repo at `<dir>/.git` with worktrees inside `<dir>` (proposed),
  vs. the `.bare`-directory convention some tools use, vs. fully bare `<dir>.git` with
  worktrees as siblings.
- Stable naming for level namespaces (filter-id hash is canonical but unreadable; a declared
  name or view name is readable but needs a rename story), and the exact ref layout within a
  level's namespace.
- Whether `josh fetch` refilters all active edges eagerly (proposed: yes — that is the point
  of sharing) or lazily on first use per worktree.
- Avoiding duplicate object storage between store and worktrees: alternates make objects
  reachable, but fetch negotiation does not account for them; candidates include josh writing
  refs directly on both sides, negotiation tips, or hardlink-friendly layouts.
- Whether a plain `git push` into the store may trigger automatic relay to the upstream via a
  hook, or relaying stays explicit in `josh push`. Leaning explicit-only: `git push` never
  having network side effects is a property worth keeping.
- How the correspondence between a level remote's native history and the filtered projection is
  established when they are independently rooted (rust-lang/miri predates any given filtered
  image and matches it only at sync points): whether josh merely relies on existing sync merges
  for common ancestry, or records/creates correspondence points itself. Either way, this cursor
  is materialization state owned by the store — deliberately not part of the view definition
  (see the views RFC, reverse views).
- CLI syntax for declaring a remote's level: positional filter as sketched vs. an explicit
  `--level` flag to keep it visually distinct from the old checkout-filter argument, and how to
  spell the reverse-direction edge (adding the wider repo second, e.g. cloning miri and adding
  rust as "origin is rust through `:=miri`"). A reverse view sidesteps the spelling in the
  common case: when the projection carries its own provenance, adding either repo second needs
  no filter argument at all.
- What `josh compose run` uses as its workspace root in a multi-worktree store.

## Future work (explicitly out of scope for v1)

- View references as worktree filter arguments (depends on the views RFC).
- Translation along paths longer than one edge (e.g. between two sibling projections: unapply
  through one filter, apply the other).
- Ephemeral worktrees for agents (`josh worktree add --detach --tmp`): auto-cleanup, naming,
  and lifecycle hooks.
- Changing a worktree's filter in place (`josh worktree set-filter`), which needs history
  translation of local branches between filters.
- A `josh worktree exec`-style helper to run a command in a fresh throwaway projection.
