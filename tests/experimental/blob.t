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

Roundtrip: blob specified by sha renders as bare hex
  $ josh-filter -p ':$added.txt=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391'
  :$added.txt=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391

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

Apply: blob referenced by sha is inserted at the destination path
  $ OID=$(printf 'big content' | git hash-object -w --stdin)
  $ josh-filter -s ":\$added.txt=$OID" master --update refs/josh/filter/master3 1> /dev/null
  $ git show josh/filter/master3:added.txt
  big content (no-eol)

Apply: referencing a non-blob sha (a commit) fails
  $ COMMIT=$(git rev-parse HEAD)
  $ josh-filter -s ":\$bad.txt=$COMMIT" master --update refs/josh/filter/bad 2> /dev/null
  [1] :$added.txt="hello world"
  [1] :$added.txt=422057123b178e433e852ef1dfee39368fb5a8ce
  [1] :[
      :/sub1
      :$added.txt="hello world"
  ]
  [1] reachable_roots
  [1] sequence_number
  [1]

Apply: referencing a nonexistent sha fails
  $ josh-filter -s ':$bad.txt=0000000000000000000000000000000000000001' master --update refs/josh/filter/bad2 2> /dev/null
  [1] :$added.txt="hello world"
  [1] :$added.txt=422057123b178e433e852ef1dfee39368fb5a8ce
  [1] :[
      :/sub1
      :$added.txt="hello world"
  ]
  [1] reachable_roots
  [1] sequence_number
  [1]
