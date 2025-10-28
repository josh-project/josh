  $ git init -q 1> /dev/null

Initial commit
  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

Apply prefix filter
  $ josh-filter -s :prefix=subtree refs/heads/master --update refs/heads/filtered
  [1] :prefix=subtree
  [1] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  * add file1

  $ git ls-tree --name-only -r refs/heads/filtered
  subtree/file1
