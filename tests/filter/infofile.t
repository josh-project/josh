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

  $ mkdir sub2
  $ echo contents1 > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ echo contents1 > unrelated
  $ git add .
  $ git commit -m "unrelated" 1> /dev/null

  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master
  21a904a6f350cb1f8ea4dc6fe9bd4e3b4cc4840b
  [2] :/sub1
  [2] :prefix=c
  [6] sequence_number
  $ git log --graph --pretty=%s josh/filter/master
  * add file2
  * add file1

  $ josh-filter -s c=:/sub1 master --update refs/josh/filter/master
  Warning: reference refs/josh/filter/master wasn't updated
  21a904a6f350cb1f8ea4dc6fe9bd4e3b4cc4840b
  [2] :/sub1
  [2] :prefix=c
  [6] sequence_number
  $ git log --graph --pretty=%s josh/filter/master
  * add file2
  * add file1

  $ josh-filter -s c=:/sub2 master --update refs/josh/filter/master
  a0d6ebb0ef3270908e83192cad2444e085f90303
  [2] :/sub1
  [2] :/sub2
  [3] :prefix=c
  [7] sequence_number
  $ git log --graph --pretty=%s josh/filter/master
  * add file3

  $ echo contents2 > sub1/file5
  $ git add sub1
  $ git commit -m "add file5" 1> /dev/null

  $ josh-filter -s c=:/sub2 master --update refs/josh/filter/master
  Warning: reference refs/josh/filter/master wasn't updated
  a0d6ebb0ef3270908e83192cad2444e085f90303
  [2] :/sub1
  [2] :/sub2
  [3] :prefix=c
  [8] sequence_number
  $ git log --graph --pretty=%s josh/filter/master
  * add file3
