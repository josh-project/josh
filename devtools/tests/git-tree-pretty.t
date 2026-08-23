  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo
  $ cd repo

  $ mkdir -p src/util
  $ echo "hello world" > src/hello.txt
  $ echo "print('hi')" > src/util/tool.py
  $ printf 'first line\nno newline' > notes.txt
  $ printf '\xffELF\x01\x02' > blob.bin
  $ ln -s hello.txt src/link.txt

  $ git add -A
  $ git commit -qm initial

  $ git-tree-pretty
  .
  ├── blob.bin
  │   ┆  <binary, 6 bytes>
  │   ┆  ff 45 4c 46 01 02                                 ·ELF··
  ├── notes.txt
  │   ┆  first line
  │   ╵  no newline
  └── src/
      ├── hello.txt
      │   ┆  hello world
      ├── link.txt
      │   ╵  hello.txt
      └── util/
          └── tool.py
              ┆  print('hi')

  $ cd ${TESTTMP}
  $ git-tree-pretty --repo repo HEAD --no-contents
  .
  ├── blob.bin
  ├── notes.txt
  └── src/
      ├── hello.txt
      ├── link.txt
      └── util/
          └── tool.py

  $ git-tree-pretty --repo repo nosuchref
  error: failed to resolve "nosuchref": couldn't parse revision: "nosuchref": The ref partially named "nosuchref" could not be found
  [1]

  $ git init -q snapshots
  $ cd snapshots
  $ printf 'ignored.txt\n' > .gitignore
  $ printf 'remove\n' > deleted.txt
  $ printf 'head\n' > staged.txt
  $ git add -A
  $ git commit -qm initial
  $ printf 'index\n' > staged.txt
  $ git add staged.txt
  $ printf 'worktree\n' > staged.txt
  $ rm deleted.txt
  $ printf 'new\n' > untracked.txt
  $ printf 'ignored\n' > ignored.txt

  $ git-tree-pretty +
  .
  ├── .gitignore
  │   ┆  ignored.txt
  ├── deleted.txt
  │   ┆  remove
  └── staged.txt
      ┆  index

  $ git-tree-pretty .
  .
  ├── .gitignore
  │   ┆  ignored.txt
  ├── staged.txt
  │   ┆  worktree
  └── untracked.txt
      ┆  new

  $ cd ${TESTTMP}

  $ git init -q empty
  $ cd empty
  $ touch placeholder
  $ git add placeholder
  $ git commit -qm initial
  $ git rm -q placeholder
  $ git commit -qm "empty tree"
  $ git-tree-pretty HEAD^{tree}
  .
