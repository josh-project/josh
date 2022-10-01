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
  $ git ls-tree --name-only -r refs/josh/filter/master 
  $ josh-filter -s c=:/libs master --update refs/josh/filter/master
  Warning: reference refs/josh/filter/master wasn't updated
  [1] :/libs
  [1] :prefix=c
  $ git ls-tree --name-only -r refs/josh/filter/master 

(note, the rest of the file consists of comments)
$ git log refs/josh/filter/master --graph --pretty=%s
* add file2
* add file1

$ git ls-tree --name-only -r refs/josh/filter/master 
c/file1
c/file2

$ git rm -r sub1
rm 'sub1/file1'
rm 'sub1/file2'
$ git commit -m "rm sub1" 1> /dev/null

$ josh-filter -s master --update refs/josh/filter/master c=:/sub1

$ git log refs/josh/filter/master --graph --pretty=%s
* rm sub1
* add file2
* add file1

$ git ls-tree --name-only -r refs/josh/filter/master 
