  $ export TESTTMP=${PWD}

Op::Blob overwrites a file that already exists at the destination path. The
filter is generative -- it ignores the input tree -- so on its own the result
contains only the blob and any sibling content is dropped. To replace a single
file while keeping its siblings, compose the blob with the exclude of the same
blob filter: the exclude drops whatever the blob would add (here a/b/c.txt) so
the blob can re-add it without a conflict, and because both sides use the
identical filter the compose stays invertible. The destination is a
multi-component path, so it exercises the split into a leaf blob followed by a
prefix.

  $ cd ${TESTTMP}
  $ git init -q repo 1> /dev/null
  $ cd repo
  $ mkdir -p a/b
  $ printf old > a/b/c.txt
  $ printf keep > a/b/d.txt
  $ printf root > top.txt
  $ git add .
  $ git commit -m init 1> /dev/null

Forward: inline content overwrites the pre-existing file, dropping siblings
  $ josh-filter ':$a/b/c.txt="new"' master --update refs/josh/inline 1> /dev/null
  $ git ls-tree -r --name-only josh/inline
  a/b/c.txt
  $ git show josh/inline:a/b/c.txt
  new (no-eol)

Forward: a blob referenced by sha overwrites the pre-existing file
  $ OID=$(printf from-sha | git hash-object -w --stdin)
  $ josh-filter ":\$a/b/c.txt=$OID" master --update refs/josh/sha 1> /dev/null
  $ git show josh/sha:a/b/c.txt
  from-sha (no-eol)

Reverse: a multi-component blob inverts to :empty (it consumes no input)
  $ josh-filter --reverse -p ':$a/b/c.txt="new"'
  :empty

Reverse: the inverse does not depend on the blob content (sha form is identical)
  $ josh-filter --reverse -p ":\$a/b/c.txt=$OID"
  :empty

Composing the blob with the exclude of the same blob keeps the siblings
  $ josh-filter ':[:exclude[:$a/b/c.txt="new"],:$a/b/c.txt="new"]' master --update refs/josh/keep 1> /dev/null
  $ git ls-tree -r --name-only josh/keep
  a/b/c.txt
  a/b/d.txt
  top.txt
  $ git show josh/keep:a/b/c.txt
  new (no-eol)
  $ git show josh/keep:a/b/d.txt
  keep (no-eol)

Composing the blob over a passthrough replaces the file while keeping siblings
  $ josh-filter ':[:/,:$a/b/c.txt="new"]' master --update refs/josh/overlay 1> /dev/null
  $ git ls-tree -r --name-only josh/overlay
  a/b/c.txt
  a/b/d.txt
  top.txt
  $ git show josh/overlay:a/b/c.txt
  new (no-eol)
  $ git show josh/overlay:a/b/d.txt
  keep (no-eol)

Reverse: the passthrough overlay inverts to identity (the blob contributes no input)
  $ josh-filter --reverse -p ':[:/,:$a/b/c.txt="new"]'
  :/

Reverse: edits to the filtered tree flow back upstream, including the blob path
  $ josh-filter ':[:exclude[:$a/b/c.txt="new"],:$a/b/c.txt="new"]' master --update refs/josh/rt 1> /dev/null
  $ git checkout -q refs/josh/rt 2> /dev/null
  $ printf edited-c > a/b/c.txt
  $ printf edited-d > a/b/d.txt
  $ git add a/b/c.txt a/b/d.txt
  $ git commit -m edit 1> /dev/null
  $ git update-ref refs/josh/rt HEAD
  $ josh-filter ':[:exclude[:$a/b/c.txt="new"],:$a/b/c.txt="new"]' --reverse master --update refs/josh/rt 1> /dev/null
  $ git show master:a/b/c.txt
  edited-c (no-eol)
  $ git show master:a/b/d.txt
  edited-d (no-eol)
