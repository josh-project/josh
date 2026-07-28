A plain `:SQUASH` view presents the entire history as a single squashed
commit. Since `:SQUASH` is identity at tree level, commits made on top of the
view unapply onto the original history with their content unchanged; the
re-filtered view is the squash of the new tip.

  $ git init -q 1>/dev/null

  $ echo a > file1
  $ git add .
  $ git commit -m "one" 1>/dev/null
  $ echo b > file2
  $ git add .
  $ git commit -m "two" 1>/dev/null

  $ josh-filter -s :SQUASH refs/heads/master --update refs/heads/filtered
  ef7dc63d9aeb1f69b5537e8fd7ab9682a03084d7

  $ git log --graph --pretty=%s refs/heads/filtered
  * two

  $ git ls-tree --name-only -r refs/heads/filtered
  file1
  file2

A commit on top of the squashed view pushes back onto the original history
  $ git checkout -q -b work refs/heads/filtered
  $ echo c > file3
  $ git add .
  $ git commit -m "add file3" 1>/dev/null
  $ git update-ref refs/heads/filtered HEAD

  $ josh-filter -s :SQUASH refs/heads/master --update refs/heads/filtered --reverse
  e947255d78b17600bb2b9036e5c86104cdf639ee

  $ git log --graph --pretty=%s refs/heads/master
  * add file3
  * two
  * one

  $ git ls-tree --name-only -r refs/heads/master
  file1
  file2
  file3

Re-filtering squashes the updated history back into a single commit
  $ josh-filter -s :SQUASH refs/heads/master --update refs/heads/refiltered
  8fe1090d633aa8413a2f04e20c0c11efe6dfc332

  $ git log --graph --pretty=%s refs/heads/refiltered
  * add file3

  $ git ls-tree --name-only -r refs/heads/refiltered
  file1
  file2
  file3
