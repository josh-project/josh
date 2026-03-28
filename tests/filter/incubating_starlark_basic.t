  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add sub1" 1> /dev/null

  $ mkdir sub2
  $ echo contents2 > sub2/file2
  $ git add sub2
  $ git commit -m "add sub2" 1> /dev/null

  $ mkdir -p st
  $ cat > st/config.star <<'EOF'
  > # Simple starlark filter: keep only sub1
  > filter = filter.subdir("sub1")
  > EOF
  $ git add st
  $ git commit -m "add starlark config" 1> /dev/null

  $ josh-filter -s :*st/config master --update refs/josh/master
  3503c2ff9f931db3e256bb6deb1a2d147e7b1c2b
  [3] :*st/config
  [3] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * add starlark config
  * add sub2
  * add sub1

  $ git checkout refs/josh/master 2> /dev/null
  $ git ls-tree -r HEAD
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfile1 (esc)
  100644 blob f34a2bbfa250847f10a4102e093b48be1d2873a1\tst/config.star (esc)
  $ tree
  .
  |-- file1
  `-- st
      `-- config.star
  
  2 directories, 2 files

