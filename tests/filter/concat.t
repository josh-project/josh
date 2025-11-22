  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null
  $ git update-ref refs/heads/from_here HEAD


  $ mkdir sub2
  $ echo contents1 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ josh-filter ":\"x\""
  9d117d96dfdba145df43ebe37d9e526acac4b17c

  $ git log --graph --pretty=%s:%H HEAD
  * add file3:667a912db7482f3c8023082c9b4c7b267792633a
  * add file2:81b10fb4984d20142cd275b89c91c346e536876a
  * add file1:bb282e9cdc1b972fffd08fd21eead43bc0c83cb8

  $ git log --graph --pretty=%s:%H FILTERED_HEAD
  * x:9d117d96dfdba145df43ebe37d9e526acac4b17c
  * x:b232aa8eefaadfb5e38b3ad7355118aa59fb651e
  * x:6b4d1f87c2be08f7d0f9d40b6679aab612e259b1

  $ josh-filter -p ":from(81b10fb4984d20142cd275b89c91c346e536876a:\"x\")"
  :"x":concat(81b10fb4984d20142cd275b89c91c346e536876a:"x")
  $ josh-filter ":from(81b10fb4984d20142cd275b89c91c346e536876a:\"x\")"
  5ec25aa6f14a75374b1caab14bf9ee9818466d4f

  $ git log --graph --pretty=%s FILTERED_HEAD
  * x
  * add file2
  * add file1
