Pattern filters run the glob crate with require_literal_leading_dot: `*` and `**` never match a
path component that starts with '.', at any depth. Literal dot-led components in the pattern are
unaffected and match normally.

  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir -p d .d .github/workflows
  $ echo hidden_root > .hidden.rs
  $ echo hidden_sub > d/.hidden.rs
  $ echo in_dot_dir > .d/x.rs
  $ echo visible > d/visible.rs
  $ echo ci > .github/workflows/ci.yml
  $ git add .
  $ git commit -m "add files" 1> /dev/null

`::**/*.rs` must exclude dot-led components at every depth: the dot-led blobs `.hidden.rs` and
`d/.hidden.rs`, and everything under the dot-led directory `.d/`:

  $ josh-filter -s "::**/*.rs" master --update refs/heads/rs
  84780ae93f90c8dfc053fe624e808af7c2187e0b
  [1] ::**/*.rs
  [1] reachable_roots
  [1] sequence_number
  $ git ls-tree -r --name-only refs/heads/rs
  d/visible.rs

A literal dot-led pattern component matches dot-led names; only wildcards refuse them:

  $ josh-filter -s "::.github/**/*.yml" master --update refs/heads/yml
  6a611b6b95846206de1f7323da3f88217d6c9984
  [1] ::**/*.rs
  [1] ::.github/**/*.yml
  [1] reachable_roots
  [1] sequence_number
  $ git ls-tree -r --name-only refs/heads/yml
  .github/workflows/ci.yml
