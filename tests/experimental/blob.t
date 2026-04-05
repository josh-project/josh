  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1> /dev/null
  $ cd repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

Roundtrip: filter spec is preserved
  $ josh-filter -p ':$added.txt="hello world"'
  :$added.txt="hello world"

Roundtrip: path with special chars is quoted
  $ josh-filter -p ':$"hello world.txt"="content"'
  :$"hello world.txt"="content"

Inverse renders as :exclude[::path]
  $ josh-filter --reverse -p ':$added.txt="hello world"'
  :exclude[::added.txt]

Apply: blob is inserted at correct path with correct content
  $ josh-filter -s ':$added.txt="hello world"' master --update refs/josh/filter/master 1> /dev/null
  $ git show josh/filter/master:added.txt
  hello world (no-eol)

Apply: compose with blob inserts blob alongside other files
  $ josh-filter -s ':[:/sub1,:$added.txt="hello world"]' master --update refs/josh/filter/master2 1> /dev/null
  $ git diff ${EMPTY_TREE}..josh/filter/master2
  diff --git a/added.txt b/added.txt
  new file mode 100644
  index 0000000..95d09f2
  --- /dev/null
  +++ b/added.txt
  @@ -0,0 +1 @@
  +hello world
  \ No newline at end of file
  diff --git a/file1 b/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file1
  @@ -0,0 +1 @@
  +contents1
