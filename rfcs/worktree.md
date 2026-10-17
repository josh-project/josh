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
moves history between the store and the upstreams. Each remote is mounted below a distinct
path in the store's composed tree; worktree filters operate over that composition.

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
  by remote configuration rather than by the checkout in front of you. The legitimate
  per-remote fact is its non-overlapping mount path in the store's composition; the checkout
  filter remains worktree state.

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
reintroduces a privileged remote instead of a composition of explicit mounted peers. The two
primitives are instead:

```
josh init [<dir>]
josh remote add <name> <url> <path-filter>
```

`josh init` creates `<dir>` containing a checkout-less repository (the *shared repo*): the josh
cache and no history yet. `josh remote add` records a remote (URL, forge config), fetches its
history, and mounts its native root below `<path-filter>`. The filter is required, must be a
non-root path-only filter such as `:/rust`, and must not overlap another remote mount. There is
no implicit `origin`: remotes have explicit names, as befits peers.

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

The protocol layer is therefore exactly where josh puts the boundary. A worktree is a linked
repository — own refs, own config, objects shared with the store via alternates — whose Git
remotes invoke a Josh-owned upload/receive adapter:

```
josh serve-worktree --namespace <worktree-level>/<remote> upload-pack
josh serve-worktree --namespace <worktree-level>/<remote> receive-pack
```

The command is the stable boundary between plain Git in the worktree and Josh's store model.
The first implementation is deliberately a thin wrapper: it validates the requested identity,
sets `GIT_NAMESPACE`, and execs Git's `upload-pack` or `receive-pack` against the store. Fetched
native refs, mounted histories, and materialized worktree refs therefore continue to use Git
namespaces in v1.

A later implementation may replace the wrapper internals with a virtual-ref server backed
directly by the synthetic superhistory. It can derive advertised refs from remote-ref gitlinks
and worktree mappings, and record received updates in the hidden supertree, without creating
per-worktree namespace refs in the store. The worktree's remote configuration and ordinary Git
UX do not change across that cutover.

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
- **The protocol adapter is evolvable.** V1 delegates protocol correctness, pack negotiation,
  quarantine, and atomic ref updates to Git through `GIT_NAMESPACE`. Future versions can serve
  the same worktree-facing refs virtually from `refs/josh/super` without changing any worktree
  repository.

Two worktrees at the same level attach to the same namespace and thus share its refs;
refiltering happens once per level, not once per worktree. Each worktree reads the store ODB
through an alternate but writes new objects to its own ODB. Parking a branch transfers the
needed objects into the store; duplicate local copies may remain and are intentionally
tolerated until explicit Josh maintenance.

### Object retention and `josh gc`

Immediate deduplication is not required for correctness. Git's fetch negotiation does not fully
account for alternates, and commits created locally remain in the worktree ODB even after a push
has transferred them to the store. V1 leaves those copies in place.

A future `josh gc` command owns maintenance across the store and every registered worktree. It
may remove a worktree object only after confirming that the object is durable in the store and
protected by a store-visible ref. A cheap pass can delete duplicate loose objects and packs
whose entire contents exist in the store; a deeper pass can repack only objects still unique to
the worktree. That design will disable automatic worktree GC so Git cannot prune or repack a
shared topology without considering the other repositories. Store GC will likewise treat
registered worktree refs as reachability roots.

### Remote mounts: disjoint native repositories in one composed tree

Every remote is a native repository with its own root. `remote add` assigns that root a mount
path in the store:

```
josh init projects
josh remote add rust https://github.com/rust-lang/rust :/rust
josh remote add miri https://github.com/rust-lang/miri :/miri
josh worktree add rust-work :/rust
josh worktree add miri-work :/miri
josh worktree add all
```

The composed tree has `rust/` and `miri/`. A `:/rust` worktree sees rust at its native root;
the identity worktree sees both mounts. Mount filters deliberately support only a single path.
`:/` is invalid for a remote because a full remote would overlap every peer. Equal paths and
ancestor/descendant paths are rejected for the same reason.

Native refs are mounted by applying a prefix filter before any worktree filter. On push, Josh
reverses that composition: it unapplies the worktree filter and the mount, sending only the
selected remote's native tree and history upstream. This makes a branch in a full worktree safe
to relay independently to several disjoint remotes.

The remotes need not share ancestry. Their mounted branches remain distinct remote-tracking
refs; a full worktree can merge branches from independent remotes when it needs one branch that
contains several mounts. Scoped worktrees normally track the remote whose mount they select.

Submodule discovery follows the same rule. The superproject keeps its declared mount, while
discovered submodule remotes default to mounts named after their submodule IDs. Those mounts do
not need to match paths inside the superproject. Inlining a gitlink remains a worktree filter,
not remote configuration.

### Sync commands: git moves history locally, josh moves it across boundaries

The worktree↔store boundary belongs to git; store↔upstream and mount/worktree translation
belong to josh:

- **Plain `git fetch` / `git push`** in a worktree exchange refs with the store's namespaces —
  always local, always safe, never on the network.
- **`josh fetch`** (anywhere inside the store) fetches each upstream into its native
  namespace, mounts it in the composed root, and materializes every active worktree filter.
  All worktrees see the update on their next `git fetch`, which `josh fetch` runs for them by
  default.
- **`josh push`** relays a branch parked in a worktree namespace out to one upstream,
  unapplying the worktree filter and that remote's mount. Other mounts are outside the selected
  remote's image and cannot leak into the push.
- **`josh pull`** is `josh fetch` plus integrating the current branch (delegating the
  merge/rebase to git, as today). **`josh changes …`** uses store-owned logical change state
  described below.

Everything else — committing, branching, rebasing, diffing, bisecting — is plain git in the
worktree, with no josh wrapper and no caveats, because it only ever touches real local filtered
history.

### Change state: shared through the synthetic superhistory

Canonical change metadata lives in the hidden `.josh/changes/` tree of `refs/josh/super`,
alongside declarative configuration and the gitlinks recording remote-ref state. A separate,
per-remote-branch changes ref exists only to retain the Git objects referenced by that metadata.
Published review commits remain ordinary refs in each actual upstream:

| Location | Scope | Purpose |
| --- | --- | --- |
| `.josh/changes/<change-id>/...` in `refs/josh/super` | store-wide logical change | canonical projections, comments, votes, outbox, targets, publication and landing status |
| `refs/josh/remotes/<remote>/changes/<branch>` | one native remote branch | keep every evolving projected revision reachable across amends |
| `refs/heads/@changes/...` in an upstream | one remote and native destination branch | published Git commits used for review |

Every metadata update advances the linear synthetic superhistory with the previous synthetic
commit as its only parent. Remote histories must never become parents of the superhistory:
otherwise retaining amended revisions would make the central graph contain the ancestry of every
remote and would not scale. Updates use compare-and-swap on `refs/josh/super`; concurrent writers
reload and merge disjoint tree updates, while conflicting edits to the same canonical change
revision fail rather than overwrite one another.

#### Per-remote-branch revision retention

Metadata names the current and historical projected revision OIDs, but a SHA in a blob does not
make its object reachable to Git GC. For each `(remote, native destination branch)`, Josh therefore
maintains:

```
refs/josh/remotes/<remote>/changes/<branch>
```

That ref is a linear retention history. Its first parent is its previous tip. When a logical
change gains a new projected revision — including an amend — Josh adds a small anchor commit as
an additional parent; the anchor's parent is the projected revision commit. The previous
retention tip keeps older anchors reachable, so amending a change cannot lose earlier revisions.

A cross-remote logical change updates one retention ref per affected remote branch. No retention
ref includes another remote's projected history, and none is a parent of `refs/josh/super`.
Metadata updates that reference new revisions publish the retention refs and the superhistory
update in one store ref transaction where supported. If they must be ordered, retention refs are
published first: an extra keepalive is harmless, while metadata pointing at an unprotected object
is not.

Comments, votes, review status, and other updates that introduce no revision OID advance only the
superhistory. `josh gc` may compact a retention ref into a fresh snapshot of anchors for revisions
still named by canonical metadata, dropping obsolete amended revisions according to retention
policy.

Keeping change state in the superhistory does not invalidate build caches. `.josh` is
automatically excluded from every user and Compose projection, and Compose caches by projected
tree. A comment-only or vote-only superhistory commit therefore produces the same projected tree
OID and reuses the existing result. Worktree history materialization may revisit such commits,
but metadata-only commits are normally elided because their visible tree is unchanged; recomputing
them is cheap enough even when it is required.

Change identity is not scoped by a physical worktree, mapping name, or local branch name.
Overlapping worktrees must observe and edit the same logical change. A full worktree and a
frontend-only worktree therefore share a frontend change even when their mappings and materialized
commit OIDs differ.

The canonical record represents a logical change as a set of native remote projections:

```
.josh/changes/update-api/
  meta.json
  projections/
    frontend/refs/heads/main/
      canonical diff or revision
    backend/refs/heads/master/
      canonical diff or revision
  publications/
    frontend/refs/heads/main/
      pushed oid, review id, landed state
    backend/refs/heads/master/
      pushed oid, review id, landed state
  comments/
  votes/
  outbox/
```

A worktree lists a change when at least one projection intersects one of its mappings. Josh
materializes only the visible projections through that worktree's mount and per-mapping filter.
Consequently, the same logical change may appear as a frontend-only commit in one worktree and a
combined frontend/backend commit in another while retaining one Change-Id, comment stream, vote
state, and publication record.

Editing a change from a narrower worktree updates only the projections visible through that
worktree. Josh reverses the mapping filter and mount into the canonical remote projection and
preserves every projection outside the worktree's image.

Stack membership and dependencies are views over the canonical records. They are computed after
projection because filtering may remove a change or a dependency in one worktree or remote while
retaining it in another. Publication likewise projects the logical stack independently for every
affected remote. Projections with no diff are skipped, and `--atomic` applies only within one
remote. Cross-remote publication cannot be atomic; successful projections are committed back into
the superhistory so retries are idempotent.

On pull, Josh updates remote-ref gitlinks in the synthetic superhistory, rematerializes the
worktree's mapped branches, and restacks visible logical changes on the new composed base. A
change is fully landed only when all affected remote projections are landed. A partially landed
change retains only its unlanded projections, and every overlapping worktree observes that same
state on its next materialization.

Removing a physical worktree or named mapping does not remove logical changes. Their metadata
remains in the synthetic history, and the per-remote-branch retention refs continue protecting
every named revision until an explicit policy allows `josh gc` to compact or collect them.

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

1. `josh init`, the store layout, native remote namespaces, and disjoint remote mounts.
2. The linear synthetic superhistory: declarative config plus every tracked remote ref as a
   gitlink, with named worktree mappings stored in its hidden `.josh` tree.
3. `josh worktree add` / `list` / `remove` as linked repositories attached to mapped,
   namespace-scoped `upload-pack` / `receive-pack` endpoints.
4. `josh fetch` and `pull` updating the superhistory and rematerializing mappings; `josh push`
   reversing mapping filters and mounts.
5. Supertree-backed, mapping-independent `josh changes` metadata; per-remote-branch revision
   retention refs; overlapping-worktree projection; and multi-remote publication.
6. Migration helper and docs.

## Open questions

- Exact store layout: bare repo at `<dir>/.git` with worktrees inside `<dir>` (proposed),
  vs. the `.bare`-directory convention some tools use, vs. fully bare `<dir>.git` with
  worktrees as siblings.
- Stable naming for worktree namespaces (filter-id hash is canonical but unreadable; a declared
  name is readable but needs a rename story), and the exact ref layout within a namespace.
- Whether `josh fetch` materializes all active worktree filters eagerly (proposed: yes — that is
  the point of sharing) or lazily on first use.
- Whether worktree registration uses continuously mirrored keepalive refs or snapshots those refs
  only while `josh gc` runs. V1 performs no cross-repository pruning.
- Whether a plain `git push` into the store may trigger automatic relay to the upstream via a
  hook, or relaying stays explicit in `josh push`. Leaning explicit-only: `git push` never
  having network side effects is a property worth keeping.
- What `josh compose run` and `shell` use as their workspace root in a multi-worktree store.

## Future work (explicitly out of scope for v1)

- View references as worktree filter arguments (depends on the views RFC).
- Translation along paths longer than one edge (e.g. between two sibling projections: unapply
  through one filter, apply the other).
- Ephemeral worktrees for agents (`josh worktree add --detach --tmp`): auto-cleanup, naming,
  and lifecycle hooks.
- Changing a worktree's filter in place (`josh worktree set-filter`), which needs history
  translation of local branches between filters.
- A `josh worktree exec`-style helper to run a command in a fresh throwaway projection.
- `josh gc`: coordinated store/worktree maintenance that evacuates durable objects to the store,
  prunes duplicate worktree objects and packs, and preserves every registered worktree ref as a
  reachability root.
