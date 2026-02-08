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
  > # With subfilter ::sub1/, the tree passed to the script has sub1 at root (just file1).
  > # Include that file in the result.
  > filter = filter.file("file1")
  > EOF
  $ git add st
  $ git commit -m "add starlark config" 1> /dev/null

  $ josh-filter -s :*st/config[::sub1/] master --update refs/josh/master
  0c7056c463e5edf79768f0b69ce5ed494d601389
  [3] :*st/config[::sub1/]
  [3] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * add starlark config
  * add sub2
  * add sub1

  $ git checkout refs/josh/master 2> /dev/null
  $ git ls-tree -r HEAD
  100644 blob 17a02eede77454427c80e6fdf862f924f9c13ae9\tst/config.star (esc)
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tsub1/file1 (esc)
  $ tree
  .
  |-- st
  |   `-- config.star
  `-- sub1
      `-- file1
  
  3 directories, 2 files

