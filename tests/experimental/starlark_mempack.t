  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add sub1" 1> /dev/null

  $ cat > config.star <<'EOF'
  > # The subfilter (:/sub1:prefix=lib) produces a tree that is written to the
  > # mempack backend when running with --pack. This test verifies that the
  > # starlark evaluation can access that mempack tree.
  > content = tree.file("lib/file1")
  > filter = filter.blob("foobar", content)
  > # else:
  > #     filter = filter.empty()
  > EOF

Apply with --pack: starlark must see the mempack tree to return the correct filter
  $ josh-filter --pack ':!config[:/sub1::file1:prefix=lib]' . --update refs/josh/master
  e915167514a08e5e19839b871c13ec581f7d9618

  $ git ls-tree -r refs/josh/master
  100644 blob 2b30477c6e975a7b6e8edec18bd617a629594681\tconfig.star (esc)
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfoobar (esc)
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tlib/file1 (esc)

The starlark filter read "lib/file1" from the mempack tree and returned
filter.subdir("sub1"), so "file1" is present at the root of the result.
  $ git show refs/josh/master:foobar
  contents1
