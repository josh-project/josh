# RFC: Change events — a portable review schema

## Summary

Restructure the josh-changes schema around *events*: self-describing, content-addressed,
immutable records of everything that happens to a change — comments, votes, revisions, and later
queue verdicts. An event carries its own provenance (author, timestamp, optional signature)
inside the hashed content; the changes-ref tree becomes a plain index of events, and the
changes-ref commit history is demoted to pure transport with no semantic content. This inverts
the one structural mistake in the current schema — provenance recovered by walking the ref's
commit history — and removes GitHub-specific bookkeeping from the portable namespace. Everything
else the current schema does right (git-native storage, content addressing, the outbox pattern,
anchor commits, Local/Remote scoping, blob-id comment anchoring, derived dependencies) is
preserved and mostly strengthened.

The goal this serves: a change and its review metadata travel as one object through staged
review contexts — local (collaboration with a coding agent), pre-publish within a team or org,
publication to open source — and the merge queue operates on the same object. Four consumers
(GUI, forge sync, stage promotion, merge queue) will lock into this schema; it must be right
before they do.

## Motivation

The current schema (`josh-changes/src/lib.rs`) stores review metadata as trees under
`refs/josh/changes/<branch>` and `refs/josh/remotes/<remote>/changes/<branch>`. Most of its
decisions are correct and are explicitly kept (see "What is preserved"). Three are not:

- **Provenance lives in the transport, not the data.** A comment blob contains only message,
  location, and threading links. Who wrote it and when is *inferred*: `read_comments` and
  `comment_author` walk the changes-ref commit history to find the commit that introduced the
  blob, and report that commit's author and time. This conflates the event (the thing that must
  travel across stage boundaries) with the local log of learning about it (which will not
  travel). Consequences compound at every boundary of the staged-review model:
  - Promoting a change to another stage requires shipping the full ref history, or authorship
    evaporates.
  - Redacting anything at a publish boundary means rewriting the commit chain that all remaining
    provenance depends on.
  - Nothing is signable: there is no self-contained record to sign, so approvals cannot carry
    verifiable weight across trust boundaries.
  - Every read is archaeology — O(ref history) per query, paid by the GUI on every refresh.

  Gerrit learned this exact lesson: NoteDb stores author and timestamp inside the note content
  and uses commits purely as transport.

- **GitHub bookkeeping sits in the portable namespace.** `gh/`, `gh_ids/`, `gh_vote_ids/` are
  top-level subtrees of every changes ref, and `vote_state_to_github_review` lives in the core
  crate. Adapter state named after one forge, stored in the schema every forge and every stage
  shares, is exactly the gravitational pull toward a GitHub-shaped model that the schema must
  resist to stay forge-agnostic.

- **Votes are too thin for the queue.** `VoteData { state: String, sha }`, one entry per user,
  silently overwritten. No history, no typed labels, no way to distinguish "a human approved"
  from "CI verified" — the distinction merge-queue admission is built on (Gerrit's Code-Review
  versus Verified labels exist for this reason). An overwrite-in-place record is also the one
  piece of the current schema that is not append-only, which complicates sync and forfeits the
  audit trail everywhere else provides.

The schema is under 2000 lines with consumers not yet locked in. This is the cheapest the
inversion will ever be.

## Design

### The event is the unit of storage

An **event** is an immutable JSON blob in canonical serialization (sorted keys, no insignificant
whitespace, UTF-8). Its **event id** is the git blob hash of the canonical bytes. Common
envelope fields:

```json
{
  "v": 1,
  "type": "comment",
  "change": "<change-id>",
  "author": "someone@example.com",
  "date": "2026-07-18T12:00:00Z",
  "payload": { ... }
}
```

- `v` — schema version of the envelope, for forward evolution.
- `type` — event type; the initial set is `comment`, `label`, `revision`. Types for queue
  verdicts and stage transitions are reserved (see below) but not part of v1.
- `author` — the acting identity (email form). Events imported from a forge carry the identity
  the adapter mapped, not a fabricated `<user>@github` address.
- `date` — RFC 3339 UTC. One time format everywhere; the current mix of epoch-second strings
  and RFC 3339 goes away.
- `payload` — per-type content.

Because provenance is inside the hashed content, the event id now covers it: two identical
messages by different authors are different events, replaying an event cannot alter its
authorship, and the id is stable across any transport. Threading links (`reply_to`,
`update_of`) reference event ids exactly as comment content-hashes are referenced today — the
links simply become stronger, since the referenced hash now pins author and time too.

### Event types

**`comment`** — payload carries `message`, optional `file`, optional anchor
`{ blob: <file-blob-oid>, start_line, end_line, start_col, end_col }`, optional `reply_to`,
optional `update_of`. Anchoring by file blob id is kept from the current schema: an anchor
survives any rebase that does not touch the file, and anchor *migration* (remapping through
diffs or through josh filters at a boundary) produces an `update_of` event rather than mutating
the original. The file path moves out of the tree path and into the payload, where it is covered
by the hash.

**`label`** — the replacement for votes. Payload: `{ label, value, revision }`.

- `label` is a name from a per-deployment set; josh defines `review` (values `approve`,
  `discuss`, `revise` — today's states) and `verified` (values `pass`, `fail`) as defaults.
- `revision` is the commit oid the judgment applies to — a label is a statement about a
  revision, not about a change in the abstract.
- Label events are append-only like everything else. Current state is a *reduction*: for each
  `(author, label)`, the latest event wins; a label event whose `revision` is no longer the
  change's tip is visible but stale. The reduction is pure code over the event set — no stored
  aggregate to invalidate.

This gives the merge queue its admission inputs (which labels, from whom, on which revision)
without any queue-specific storage, and it restores vote history and the audit trail.

**`revision`** — records that a commit oid became a revision of the change, with its base:
payload `{ commit, base }`. This replaces the `diffs/<change-id>` tip+base blob, and replaces
`read_revisions`'s history-walking reconstruction with an enumeration. The anchor-commit
mechanism (below) keeps the recorded commits reachable.

**Reserved types** (defined here so the envelope accommodates them, implemented later):
`queue` — verdicts written by the merge queue (admission, batch membership, CI result, merge);
`stage` — a change crossing a review-stage boundary, recording origin scope and policy applied.
Both are ordinary events: the queue and the promotion machinery are writers and readers of the
same schema, not owners of side tables.

### The tree is an index; the ref history is transport

Layout of a changes ref's tree:

```
events/<change-id>/<event-id>          event blob
sigs/<change-id>/<event-id>/<signer>   detached signature over the event's canonical bytes
outbox/<change-id>/<event-id>          events authored locally, pending post to this scope's remote
forge/<forge-name>/...                 adapter-owned, opaque to josh-changes
```

- **Readers read the tip tree only.** All semantics — comments, label state, revisions — are a
  function of the event set at the ref's tip. No reader walks ref history for meaning; reads
  are O(events of one change), not O(ref history).
- **The commit chain is pure transport.** Each write is still a commit (append-only, fetchable,
  auditable), and the anchor-commit device from `store_diff_data` is kept: revision events
  parent the ref update on an empty-tree commit whose parent is the recorded revision, so
  pushing the changes ref carries the code it discusses. But deleting or rewriting the chain
  loses nothing semantic — which is precisely what makes redaction and partial transport safe.
- **Concurrent writers merge by union.** The event set is a grow-only set keyed by content hash
  (a G-set CRDT). Two refs that diverged merge by tree union with no conflicts possible;
  "merge" of changes refs becomes a defined, mechanical operation, which the current
  overwrite-in-place vote records make impossible.
- **The `outbox/` unifies.** Today comments and votes have parallel outbox subtrees with
  parallel cleanup code. An outbox entry is now just an event id awaiting post; the
  echo-detection cleanup (drop the outbox entry when the event is observed coming back from the
  remote) works uniformly for every event type, present and future.

### Signatures

A signature is a detached blob at `sigs/<change-id>/<event-id>/<signer>`, signing the event's
canonical bytes (format: SSH signatures, as used by `git commit -S` with `gpg.format=ssh`;
exact envelope in Open questions). Signatures are separate from the event so that signing does
not change the event id, additional signatures can accrue later (a stage boundary may re-attest
imported events), and unsigned operation remains first-class — local and team stages may not
care. What signatures buy at trust boundaries: a `label` event of `review=approve` that crossed
two stages can still be verified against the author's key, so traveling approvals can carry
weight instead of being hearsay.

### Stage boundaries and redaction

Promotion of a change from one scope to another (local → team remote, internal → public) copies
event blobs between changes refs — ids, provenance, and signatures survive verbatim because they
are content, not history. What crosses is governed by a per-boundary **policy**: a predicate
over events (by type, label name, author, date). The default is conservative — events do not
cross a boundary unless the policy admits them — because private review (especially the
local/agent stage) will contain material never meant for wider audiences, and the first
embarrassing leak would define the tool.

Redaction is *omission*: the promoted ref simply lacks the withheld events. A withheld event
referenced by an admitted one (`reply_to` across the boundary) leaves a dangling event id —
visible as "a redacted event existed here", which is honest and acceptable. Tombstone events
making redaction explicit are an open question, not v1.

Because a policy is a predicate over a git tree, boundary policies can eventually be expressed
as josh filters over the `events/` tree — the same machinery governing what code crosses a
boundary governing what metadata crosses it. That unification is future work, not a v1
requirement; v1 policies are code.

### The forge namespace

Everything an adapter needs to remember lives under `forge/<forge-name>/`, opaque to
josh-changes: the GitHub adapter's PR snapshots (today `gh/`), event-id-to-node-id maps (today
`gh_ids/`, `gh_vote_ids/`), cursors, whatever it wants. `vote_state_to_github_review` and every
other GitHub-specific mapping moves to `josh-github-changes`. The rule this encodes: the
portable schema may not name a forge; an adapter may not write outside its namespace. Sync
logic itself simplifies — an adapter imports remote activity as events (with real provenance),
exports outbox events, and keeps its correspondence tables privately.

### What is preserved

Explicitly unchanged, because the current design got them right:

- **Git trees under refs as the substrate** — portable, offline, host-independent, moves with
  fetch/push.
- **Content addressing as identity** — extended, not replaced: the hash now also covers
  provenance.
- **The outbox pattern with echo-detection cleanup** — generalized to all event types.
- **Anchor commits for transport reachability** of discussed code.
- **`ChangesRef` Local/Remote scoping and no cross-ref fusion** — each ref one writer in
  practice, merging views is the reader's job (and is now well-defined via set union).
- **File-blob-id comment anchoring.**
- **Derived change dependencies** from Change-Id trailers of contributing commits — never
  stored, never stale.
- **Change identity via Change-Id trailer**, and the Gerrit-style push ref surface
  (`refs/for/…`, `refs/publish/for/…`).

## Migration

None. Nobody relies on the current format; the new schema replaces it outright. Existing
changes refs are discarded (local scopes are re-derived by `josh sync`, remote scopes are
repopulated from the forge by the adapter). No converter, no dual-read, no version detection —
the `v` field in the envelope exists for *future* evolution, not for reading the past.

## Implementation order

1. Event model in josh-changes: canonical serialization, envelope, `comment`/`label`/`revision`
   types, tip-tree readers, unified outbox. This deletes the history-walking readers and the
   old tree layout in one step.
2. Forge namespace: move GitHub bookkeeping under `forge/github/`, relocate mappings into
   `josh-github-changes`.
3. Consumers: GUI and CLI read the event set; label reduction replaces vote reads.
4. Signatures: detached sig writing/verification, surfaced in GUI and CLI.
5. Stage promotion: copy-with-policy between scopes; policies as code.
6. *(future)* `queue` and `stage` event types with the merge queue; policies as filters over
   the events tree.

## Open questions

- Canonical JSON details: adopt RFC 8785 (JCS) wholesale, or specify the minimal subset
  (sorted keys, no whitespace, NFC strings) locally.
- Signature envelope: raw SSH signature blobs versus a small JSON wrapper recording key id and
  algorithm; where verification keys live (per-repo allowed-signers file, forge-published keys,
  both).
- Identity model: emails as author identity are convenient and match git, but stages that
  require verifiable identity may want key fingerprints as the primary identity with email as
  display data.
- Agent participants: agents are expected to be first-class event authors (an agent-addressed
  reply delegates a review comment to an agent, whose resulting `revision` and reply events are
  authored as the agent, with full transparency about what context drove them). Open: how agent
  identity is distinguished from human identity in `author` (so label reductions can express
  "approved by a human"), whether a delegated event links to the delegating event explicitly
  (an `on_behalf_of`/delegation reference in the envelope), and whether the agent's model and
  configuration belong in the payload as attestable provenance.
- Tombstones: whether redaction-by-omission needs an explicit marker event type, or dangling
  references are enough.
- Whether `revision` events subsume the `@changes/<branch>/<author>/<change-id>` server-side
  ref layout or that remains a parallel (proxy-facing) surface.
- Label set governance: fixed per deployment, per view (once the views RFC lands), or per
  target branch.
- Judgments under filtering: when participants review a change through a filtered view, a label
  is a statement about a *projection* of a revision. Open: whether `label` payloads record the
  filter under which the judgment was made, and whether queue admission then becomes
  coverage-based (the approvals' filters must jointly cover the change's diff).

## Future work (explicitly out of scope for v1)

- `queue` and `stage` event types and the merge queue built on them.
- Boundary policies expressed as josh filters over the events tree.
- Anchor migration events written automatically when a file's blob changes (remapping through
  diffs or through filters at a boundary).
- Cross-stage identity attestation (a stage re-signing imported events it has verified).
