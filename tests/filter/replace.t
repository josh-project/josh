
  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1>/dev/null
  $ cd repo

  $ echo "hello world" > hw.txt
  $ mkdir subdir
  $ echo "hello moon" > subdir/hw.txt

  $ git add .
  $ git commit -m initial
  [master (root-commit) 79f224d] initial
   2 files changed, 2 insertions(+)
   create mode 100644 hw.txt
   create mode 100644 subdir/hw.txt

  $ git-tree-pretty refs/heads/master
  .
  ├── hw.txt
  │   ┆  hello world
  └── subdir/
      └── hw.txt
          ┆  hello moon

  $ josh-filter -p ':replace("hello":"bye","^(?P<l>.*(?m))$":"$l!")'
  :replace(
      "hello":"bye"
      "^(?P<l>.*(?m))$":"$l!"
  )
  $ josh-filter --update refs/heads/filtered ':replace("hello":"bye","(?m)^(?P<l>.+)$":"$l!")'
  f44b7f09eac089146c824d1f149b3b155e7cda50

  $ git-tree-pretty refs/heads/filtered
  .
  ├── hw.txt
  │   ┆  bye world!
  └── subdir/
      └── hw.txt
          ┆  bye moon!

  $ josh-filter --update refs/heads/filtered --reverse ':replace("hello":"bye","(?m)^(?P<l>.+)$":"$l!")'
  79f224d32bbdf7dcec1b488336f6c0baa4712138

  $ git-tree-pretty refs/heads/master
  .
  ├── hw.txt
  │   ┆  hello world
  └── subdir/
      └── hw.txt
          ┆  hello moon

  $ josh-filter --update refs/heads/filtered ':[xdir=:/subdir,:replace("hello":"bye","(?m)^(?P<l>.+)$":"$l!")]'
  80d073c7484b37d564025d7e760e4511ca6d0785
  $ git-tree-pretty refs/heads/filtered
  .
  ├── hw.txt
  │   ┆  bye world!
  └── xdir/
      └── hw.txt
          ┆  hello moon
