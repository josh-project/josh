  $ git init 1> /dev/null

  $ mkdir a
  $ echo contents1 > a/file1
  $ echo contents1 > a/file2
  $ git add a

  $ mkdir b
  $ echo contents1 > b/file1
  $ git add b

  $ mkdir -p c/d
  $ echo contents1 > c/d/file1
  $ git add c
  $ git commit -m "add files" 1> /dev/null

  $ git log --graph --pretty=%s
  * add files

  $ josh-filter -s :nosuch=filter master --update refs/josh/filtered
  ERROR: Invalid filter: ":nosuch"
  
  Note: use forward slash at the start of the filter if you're
  trying to select a subdirectory:
  
  :/nosuch
  
  [1]

  $ git ls-tree --name-only -r refs/josh/filtered
  fatal: Not a valid object name refs/josh/filtered
  [128]
