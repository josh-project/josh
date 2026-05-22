# Migration guide

This page covers breaking changes introduced in recent releases and how to adapt to them.

## CRLF line endings in gpgsig headers preserved

*Applies when upgrading from r24.10.04 to r26.04.19.*

Josh previously accidentally normalized `\r\n` line endings to `\n` inside the
`gpgsig` header of commit objects. This was a bug — git treats gpgsig values as opaque
bytes, and the standard format uses `\n` throughout — and has been fixed. Josh now preserves
whichever line endings are present in the original commit.

**What changed:** If your upstream history contains commits with `\r\n` in their gpgsig
headers, the filtered commit hashes will differ from those produced by the old version.

**How to restore the old behavior:** Use the `gpgsig="norm-lf"` meta option if you need
to reproduce a history that was created with the old normalization:

```
:~(gpgsig="norm-lf")[:/your/filter]
```

See the [gpgsig option](../reference/filters.md#gpgsig-option) for details.

## Trivial merges removed by default

*Applies when upgrading from r24.10.04 to r26.04.19.*

Josh previously kept all merge commits in the filtered history, even when the filtered tree
of a merge commit was identical to its first parent's tree (a "trivial merge"). Trivial merges
are now removed by default during history simplification.

**What changed:** Filtered histories produced by the same filter may differ from those
produced by older versions of Josh if the upstream history contains trivial merges.
The internal cache version has been bumped, so all results will be recomputed on the
first run after upgrading.

**How to restore the old behavior:** Wrap your filter with the `history="keep-trivial-merges"`
meta option:

```
:~(history="keep-trivial-merges")[:/your/filter]
```

See [Filter options](../reference/filters.md#history-option) for details.

## `:join` filter removed

*Applies when upgrading from r24.10.04 to r26.04.19.*

The `:join` filter has been removed. It was a limited alternative to using `--reverse`/push
for reconstructing upstream history from a filtered view.

**How to migrate:** Use `josh-filter --reverse` or `josh push` to write changes back to
the upstream repository.

## josh-ui (web UI) removed

*Applies when upgrading from r24.10.04 to r26.04.19.*

The `/ui` endpoint and the `josh-ui` component have been removed from the project.

**How to migrate:** There is no direct replacement. Remove any links or integrations that
pointed to the `/ui` endpoint.

## `:rev` filter syntax changed; `:from` and `:concat` removed

*Applies when upgrading from r24.10.04 to r26.04.19.*

The `:rev` filter now requires an explicit match operator before each SHA, and the
zero-SHA shorthand for the default case has been replaced by `_`.

**What changed:**

The old syntax used a bare SHA for each entry:
```
:rev(sha0:filter0,sha1:filter1,0000000000000000000000000000000000000000:default_filter)
```
The new syntax requires an operator prefix and uses `_` for the default case:
```
:rev(<=sha0:filter0,<=sha1:filter1,_:default_filter)
```

Available operators:

- `<=sha` — matches the commit and all its ancestors (inclusive); equivalent to the old
  bare-SHA behavior
- `<sha` — matches only strict ancestors (excludes the named commit itself)
- `==sha` — matches only the exact commit

**How to migrate `:rev` filters:**

- Replace each bare `sha:filter` entry with `<=sha:filter`
- Replace `0000000000000000000000000000000000000000:filter` with `_:filter`

**`:from` removed:**

The `:from(sha:filter)` filter has been removed. It kept the original history up to and
including `sha` unchanged and applied `filter` to all later commits.

Equivalent `:rev` expression:
```
:rev(<=sha:/,_:filter)
```

**`:concat` removed:**

The `:concat(sha:filter)` filter has been removed. It was the internal building block of
`:from` and sometimes appeared in normalized filter output (e.g. from `josh-filter -p`).
Filters of the form `filter:concat(sha:filter)` are equivalent to `:from(sha:filter)` and
should be migrated to:
```
:rev(<=sha:/,_:filter)
```

## Cache location changed

*Applies when upgrading from r24.10.04 to r26.04.19.*

The local sled cache is now stored at `.git/josh/cache` instead of `.git/josh`.

**What changed:** On the first run after upgrading, Josh will not find any existing cache
entries and will recompute all filter results from scratch. This is a one-time cost.

**No action required** for most users - the cache will be rebuilt automatically. If you
have scripts or tooling that reference `.git/josh` directly (e.g. to delete or back up
the cache), update them to point to `.git/josh/cache` instead.
