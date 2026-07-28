A `:SQUASH` entry in a `:rev()` filter squashes the commits it matches out of
the filtered history: none of them may appear in the output. Each one is mapped
to the surviving filtered parent with the largest sequence number (or to zero
when nothing survives). In particular, the whole squashed region between an
identity-preserved subtree lineage and the first commit after the cutoff must
collapse away, even when the boundary merges are reachable only through second
parents of further merges (the rollup/bors style indirection used in
rust-lang/rust), and the commits after the cutoff must attach directly to the
subtree lineage.

  $ git init -q 1>/dev/null

Main branch with a file outside the subtree
  $ echo rust > rustfile
  $ git add .
  $ git commit -m "m1" 1>/dev/null

Standalone subtree lineage as an orphan branch
  $ git checkout -q --orphan sub
  $ git rm -q -rf .
  $ echo subcontent > subfile
  $ git add .
  $ git commit -m "s1" 1>/dev/null
  $ export SUBTREE_TIP=$(git rev-parse HEAD)

Subtree merge on a side branch (subtree files moved under subtree/)
  $ git checkout -q master
  $ git checkout -qb syncbr
  $ git merge sub --allow-unrelated-histories --no-commit 1>/dev/null
  Automatic merge went well; stopped before committing as requested
  $ mkdir subtree
  $ git mv subfile subtree/
  $ git commit -m "sync merge" 1>/dev/null

The sync merge lands on master through two levels of merge indirection
  $ git checkout -q master
  $ git checkout -qb rollup
  $ git merge --no-ff syncbr -m "rollup merge" 1>/dev/null
  $ git checkout -q master
  $ git merge --no-ff rollup -m "bors merge" 1>/dev/null
  $ export R=$(git rev-parse HEAD)

A change after the elision cutoff
  $ echo post > subtree/post
  $ git add .
  $ git commit -m "post cutoff change" 1>/dev/null

  $ export FILTER=":~(history=\"keep-trivial-merges,no-splice\")[:rev(<=$SUBTREE_TIP:prefix=subtree,<=$R:SQUASH)]:/subtree"

The filtered history is the identity-preserved subtree lineage with the
post-cutoff commit attached directly to it; the elided region leaves no trace
  $ josh-filter -s "$FILTER" refs/heads/master --update refs/heads/filtered
  ddbbceeab0f8a545c505cccb24ae71b76cdc4f67
  [2] :/subtree
  [2] :~(
      history="keep-trivial-merges,no-splice"
  )[
      :rev(<=104346ac7daf00a08bef19a999e4c7601aae519a:prefix=subtree,<=75a11dcdd41d68e57d9d9f07862bd284a99da6f0:SQUASH)
  ]
  [8] reachable_roots
  [8] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  * post cutoff change
  * s1

The subtree commit is preserved with its original id and is the direct parent
of the post-cutoff commit
  $ test $(git rev-parse refs/heads/filtered~1) = $SUBTREE_TIP && echo preserved
  preserved

  $ git ls-tree --name-only -r refs/heads/filtered
  post
  subfile

Squashing away a merge that is the only join between two preserved lineages
would disconnect one of them: the filter fails loudly instead
  $ git checkout -q --orphan sub2
  $ git rm -q -rf .
  $ echo other > otherfile
  $ git add .
  $ git commit -m "c1" 1>/dev/null
  $ export SUB2_TIP=$(git rev-parse HEAD)
  $ git checkout -q master
  $ git merge sub2 --allow-unrelated-histories --no-commit 2>/dev/null 1>/dev/null
  $ mkdir subtree2
  $ git mv otherfile subtree2/
  $ git commit -m "second sync merge" 1>/dev/null
  $ export R2=$(git rev-parse HEAD)

  $ josh-filter -s ":~(history=\"keep-trivial-merges,no-splice\")[:rev(<=$SUBTREE_TIP:prefix=subtree,<=$SUB2_TIP:prefix=subtree2,<=$R2:SQUASH)]" refs/heads/master --update refs/heads/broken
  [2] :/subtree
  [2] :~(
      history="keep-trivial-merges,no-splice"
  )[
      :rev(<=104346ac7daf00a08bef19a999e4c7601aae519a:prefix=subtree,<=64eeef227e94b848388854ea5e8b86a043c7ac12:prefix=subtree2,<=e18fef9705f6a52d6fc63f8b98aa25a45f8866b7:SQUASH)
  ]
  [2] :~(
      history="keep-trivial-merges,no-splice"
  )[
      :rev(<=104346ac7daf00a08bef19a999e4c7601aae519a:prefix=subtree,<=75a11dcdd41d68e57d9d9f07862bd284a99da6f0:SQUASH)
  ]
  [11] reachable_roots
  [11] sequence_number
  ERROR: cannot squash e18fef9705f6a52d6fc63f8b98aa25a45f8866b7: its filtered parents 8d43373df2033c39f110dd8066eb4a78e9d4866d and 308067b73b0fa23ba642aa27603e6b6b4a63dbf4 do not descend from one another. `:SQUASH` can only preserve a single lineage
  [1]
