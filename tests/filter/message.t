  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q testrepo 1> /dev/null
  $ cd testrepo

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "original message 1" 1> /dev/null

  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "original message 2" 1> /dev/null

  $ echo contents3 > file3
  $ git add file3
  $ git commit -m "original message 3" 1> /dev/null

Test that message rewriting works
  $ josh-filter ':"new message"' --update refs/josh/filter/master master
  5f6f6e08a73a44279f4c80bd928430663c7ebbb2
  $ git log --pretty=%s josh/filter/master
  new message
  new message
  new message
  $ git log --pretty=%s master
  original message 3
  original message 2
  original message 1

  $ cd ${TESTTMP}
  $ git init -q testrepo2 1> /dev/null
  $ cd testrepo2

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "commit with {tree} and {commit}" 1> /dev/null

Test that message rewriting with template variables works
  $ josh-filter ':"Message: {tree} {commit}"' --update refs/josh/filter/master master
  025b01893026c240e56c95e6e8f1659aa417581e
  $ git log --pretty=%s josh/filter/master
  Message: 3d77ff51363c9825cc2a221fc0ba5a883a1a2c72 8e125b48e2286c74bf9be1bbb8d3034a7370eebc
  $ git cat-file commit josh/filter/master | grep -A 1 "^$"
  
  Message: 3d77ff51363c9825cc2a221fc0ba5a883a1a2c72 8e125b48e2286c74bf9be1bbb8d3034a7370eebc

