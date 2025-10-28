  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ cd ${TESTTMP}
  $ git init -q app 1> /dev/null
  $ cd app
  $ git commit -m "init" --allow-empty 1> /dev/null
  $ git submodule add ../libs 2> /dev/null
  $ git submodule status
   bb282e9cdc1b972fffd08fd21eead43bc0c83cb8 libs (heads/master)

  $ git commit -m "add libs" 1> /dev/null

  $ git log --graph --pretty=%s
  * add libs
  * init

  $ josh-filter -s :/libs master --update refs/josh/filter/master
  [1] :/libs
  [2] sequence_number
  $ git ls-tree --name-only -r refs/josh/filter/master 
  $ josh-filter -s c=:/libs master --update refs/josh/filter/master
  Warning: reference refs/josh/filter/master wasn't updated
  [1] :/libs
  [1] :prefix=c
  [2] sequence_number
  $ git ls-tree --name-only -r refs/josh/filter/master 
