  $ git init -q 1> /dev/null

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

Filter HEAD — only file1 is present
  $ josh-filter :/sub1 refs/heads/master --update refs/heads/from_head 1>/dev/null

  $ git ls-tree --name-only -r refs/heads/from_head
  file1

Add an untracked file to the working tree (not staged, not committed)
  $ echo contents2 > sub1/file2

Filter using "." — should capture the working tree including the untracked file
  $ josh-filter :/sub1 . --update refs/heads/from_dot 1>/dev/null

  $ git ls-tree --name-only -r refs/heads/from_dot
  file1
  file2
