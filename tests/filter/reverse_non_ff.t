A reverse apply rebuilds the input reference from the pushed filtered state.
If the input reference gained commits since the filtered reference was created
(a concurrent commit landing between pull and push), those commits are not
represented in the pushed state and the rebuilt input would discard them:
refuse the update like a non-fast-forward git push, unless --force is given.

  $ git init -q 1>/dev/null

  $ mkdir sub
  $ echo a > sub/file1
  $ echo r > rustfile
  $ git add .
  $ git commit -m "initial" 1>/dev/null

  $ josh-filter -s :/sub refs/heads/master --update refs/heads/filtered
  5886bbc38681f63608dc7bb09044b2f46f092eae
  [1] :/sub
  [1] reachable_roots
  [1] sequence_number

Work on the filtered side
  $ git checkout -q -b work refs/heads/filtered
  $ echo b > file2
  $ git add .
  $ git commit -m "filtered side work" 1>/dev/null
  $ git update-ref refs/heads/filtered HEAD

Meanwhile a concurrent commit lands on master
  $ git checkout -q master
  $ echo c > sub/file3
  $ git add .
  $ git commit -m "concurrent master work" 1>/dev/null
  $ export RACED_MASTER=$(git rev-parse master)

Pushing without re-pulling first is refused
  $ josh-filter -s :/sub refs/heads/master --update refs/heads/filtered --reverse
  [2] :/sub
  [5] reachable_roots
  [5] sequence_number
  ERROR: refusing non-fast-forward update of refs/heads/master -- it contains commits that the reverse apply would discard. Re-apply the filter and rebase the filtered changes onto the result, or pass --force
  [1]

  $ test $(git rev-parse master) = $RACED_MASTER && echo master-untouched
  master-untouched

With --force the update happens and the concurrent commit is discarded
  $ josh-filter -s :/sub refs/heads/master --update refs/heads/filtered --reverse --force
  cae93bf6a6f602f2777c66ce12a76065b21d30f9
  [2] :/sub
  [5] reachable_roots
  [5] sequence_number

  $ git log --graph --pretty=%s refs/heads/master
  * filtered side work
  * initial

After a proper pull (re-filter) the same push fast-forwards cleanly
  $ git update-ref refs/heads/master $RACED_MASTER
  $ josh-filter -s :/sub refs/heads/master --update refs/heads/filtered2
  6436bdb983b15392190096cc9dd832af2ae017ed
  [2] :/sub
  [5] reachable_roots
  [5] sequence_number
  $ git checkout -q -b work2 refs/heads/filtered2
  $ echo d > file4
  $ git add .
  $ git commit -m "rebased filtered work" 1>/dev/null
  $ git update-ref refs/heads/filtered2 HEAD
  $ josh-filter -s :/sub refs/heads/master --update refs/heads/filtered2 --reverse
  0b1cb14b07f000ffbed470ae7abc3c698ebb7781
  [2] :/sub
  [5] reachable_roots
  [5] sequence_number

  $ git log --graph --pretty=%s refs/heads/master
  * rebased filtered work
  * concurrent master work
  * initial
