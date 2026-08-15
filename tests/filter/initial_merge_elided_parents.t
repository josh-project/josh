Empty-parent pruning at initial merges must not count elided (zero-mapped)
parents: a merge of a branch that never touched the filtered path with a branch
whose filtered image is an empty-tree commit (a real deletion of the filtered
directory) is not a merge of unrelated histories -- pruning its only surviving
parent would disconnect the deletion history.

  $ git init -q 1>/dev/null

  $ echo other > otherfile
  $ git add .
  $ git commit -m "r0" 1>/dev/null

A branch where sub/ is created and later deleted
  $ git checkout -q -b subwork
  $ mkdir sub
  $ echo a > sub/file
  $ git add .
  $ git commit -m "add sub/file" 1>/dev/null
  $ git rm -q -r sub
  $ git commit -m "delete sub" 1>/dev/null

A branch that never touched sub/
  $ git checkout -q master
  $ echo edit >> otherfile
  $ git add .
  $ git commit -m "edit otherfile" 1>/dev/null

  $ git merge -q --no-ff subwork -m "merge subwork" 1>/dev/null
  $ mkdir sub
  $ echo b > sub/newfile
  $ git add .
  $ git commit -m "re-add sub/newfile" 1>/dev/null

The filtered history stays connected through the empty-tree deletion commit
  $ josh-filter -s :/sub refs/heads/master --update refs/heads/filtered
  5c47a297501249097cd14f23f2c2cc3953dda316
  [5] :/sub
  [6] reachable_roots
  [6] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  * re-add sub/newfile
  * delete sub
  * add sub/file

  $ git log --pretty=%s --max-parents=0 refs/heads/filtered
  add sub/file

Pruning must also never leave a commit parentless: a merge of two unrelated
branches whose filtered images are both empty-tree commits keeps its parents,
so both branches' histories stay connected
  $ git checkout -q --orphan o1 master
  $ git rm -q -rf .
  $ mkdir sub
  $ echo x > sub/x
  $ git add .
  $ git commit -m "o1: add sub/x" 1>/dev/null
  $ git rm -q -r sub
  $ git commit -m "o1: delete sub" 1>/dev/null

  $ git checkout -q --orphan o2 master
  $ git rm -q -rf .
  $ mkdir sub
  $ echo y > sub/y
  $ git add .
  $ git commit -m "o2: add sub/y" 1>/dev/null
  $ git rm -q -r sub
  $ git commit -m "o2: delete sub" 1>/dev/null

  $ git checkout -q o1
  $ git merge -q --allow-unrelated-histories --no-ff o2 -m "join o1 and o2" 1>/dev/null
  $ mkdir sub
  $ echo z > sub/z
  $ git add .
  $ git commit -m "re-add sub/z" 1>/dev/null

  $ josh-filter -s :/sub refs/heads/o1 --update refs/heads/filtered2
  c4bf54531bc28f9263f7d682c1699bc6f5097b32
  [11] :/sub
  [16] reachable_roots
  [16] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered2
  * re-add sub/z
  *   join o1 and o2
  |\  
  | * o2: delete sub
  | * o2: add sub/y
  * o1: delete sub
  * o1: add sub/x
