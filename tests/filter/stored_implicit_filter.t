  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir st
  $ cat > st/config.josh <<EOF
  > ::sub2/subsub/
  > ::sub1/
  > EOF
  $ git add st
  $ git commit -m "add st" 1> /dev/null

  $ josh-filter -s :+st/config master --update refs/josh/master
  6f43d8ee3bbf33e24fa190605ed12c47e6ede762
  [2] :+st/config
  [2] :[
      ::sub1/
      ::sub2/subsub/
  ]
  [3] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * add st
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  |-- st
  |   `-- config.josh
  |-- sub1
  |   `-- file1
  `-- sub2
      `-- subsub
          `-- file2
  
  5 directories, 3 files

