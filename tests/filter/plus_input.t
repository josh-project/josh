  $ git init -q 1> /dev/null

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

Stage file2 but do not commit it
  $ echo contents2 > sub1/file2
  $ git add sub1/file2

Also create file3 in the working tree but do not stage it
  $ echo contents3 > sub1/file3

Filter using "+" — should include file1 and file2 (staged), but not file3 (unstaged)
  $ josh-filter :/sub1 + --update refs/heads/from_index 1>/dev/null

  $ git ls-tree --name-only -r refs/heads/from_index
  file1
  file2
