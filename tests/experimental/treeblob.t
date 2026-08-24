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

Roundtrip: filter spec is preserved
  $ josh-filter -p ':#version.txt[:/sub1]'
  :#version.txt[:/sub1]

Roundtrip: :# spec is preserved
  $ josh-filter -p ':#version.txt'
  :#version.txt

Roundtrip: :#/path sugar canonicalizes to the expanded form
  $ josh-filter -p ':#/sub1'
  :#sub1:/sub1

Roundtrip: :#/a/b sugar with multi-segment path canonicalizes correctly
  $ josh-filter -p ':#/a/b'
  :#a/b:/a/b

Roundtrip: :& spec is preserved
  $ josh-filter -p ':&version.txt'
  :&version.txt

TreeId via stored filter: gitlink carries the tree OID of the subfilter result
  $ cat > filter.josh <<'EOF'
  > :#version[:/sub1]
  > EOF
  $ git add filter.josh
  $ git commit -m "add filter" 1> /dev/null
  $ josh-filter -s :+filter master --update refs/josh/master 1> /dev/null
  $ git ls-tree refs/josh/master version | cut -d' ' -f1-2
  160000 commit
  $ [ "$(git rev-parse refs/josh/master:version)" = "$(git rev-parse master:sub1)" ] && echo "match"
  match

Reverse: a treeid inverts to :empty (it fabricates a gitlink and consumes no input)
  $ josh-filter --reverse -p ':#version[:/sub1]'
  :empty

Apply: composing treeid groups across sibling directories keeps every entry
(regression: an insert/treeid group must invert to :empty, otherwise the compose
uniqueness handling subtracts later groups away)
  $ josh-filter -s ':[:#x/va[:/sub1],:#y/vb[:/sub2]]' master --update refs/josh/manytree 1> /dev/null
  $ git ls-tree -r --name-only refs/josh/manytree
  x/va
  y/vb

Apply: identical treeids with the same basename in different directories all survive
(regression: the shared leaf treeid must not be hoisted out of the compose, or the
resulting :prefix compose collapses every branch but the first)
  $ josh-filter -s ':[:#x/v[:/sub1],:#y/v[:/sub1]]' master --update refs/josh/sametree 1> /dev/null
  $ git ls-tree -r --name-only refs/josh/sametree
  x/v
  y/v

Deref: path not found is treated as nop (reference is updated, path absent from output)
  $ josh-filter ':#version.txt' master --update refs/josh/noptest 1> /dev/null
  $ git rev-parse --verify refs/josh/noptest > /dev/null && echo "updated"
  updated
  $ git show refs/josh/noptest:version.txt 2>/dev/null || echo "not present"
  not present

Deref: gitlink carrying a valid tree SHA resolves and inserts at path
  $ git update-index --add --cacheinfo 160000,$(git rev-parse master:sub1),ptr
  $ git commit -m "add ptr" 1> /dev/null
  $ josh-filter -s ':#ptr' master --update refs/josh/deref 1> /dev/null
  $ git show refs/josh/deref:ptr/file1
  contents1

Deref: a non-gitlink entry is an error (reference is not updated)
  $ printf 'not-a-reference\n' > bad_ptr.txt
  $ git add bad_ptr.txt
  $ git commit -m "add bad_ptr" 1> /dev/null
  $ josh-filter ':#bad_ptr.txt' master --update refs/josh/badtest 2>&1; echo "exit:$?"
  *:#: expected gitlink at path: bad_ptr.txt (glob)
  exit:1
  $ git rev-parse --verify refs/josh/badtest 2>/dev/null || echo "not updated"
  not updated

Deref: gitlink with object not in repo is an error (reference is not updated)
  $ git update-index --add --cacheinfo 160000,0000000000000000000000000000000000000001,ghost_ptr
  $ git commit -m "add ghost_ptr" 1> /dev/null
  $ josh-filter ':#ghost_ptr' master --update refs/josh/ghosttest 2>&1; echo "exit:$?"
  *:#: object not found in repo: 0000000000000000000000000000000000000001 (glob)
  exit:1
  $ git rev-parse --verify refs/josh/ghosttest 2>/dev/null || echo "not updated"
  not updated

ObjectRef: stores the tree OID of sub1 in a gitlink at sub1
  $ josh-filter -s ':&sub1' master --update refs/josh/treeref 1> /dev/null
  $ git ls-tree refs/josh/treeref sub1 | cut -d' ' -f1-2
  160000 commit
  $ [ "$(git rev-parse refs/josh/treeref:sub1)" = "$(git rev-parse master:sub1)" ] && echo "match"
  match

ObjectRef + ObjectDeref round-trip: tree entry restored
  $ josh-filter ':#sub1' refs/josh/treeref --update refs/josh/roundtrip 1> /dev/null
  $ git show refs/josh/roundtrip:sub1/file1
  contents1

ObjectRef + :#/path sugar round-trip: tree entry restored via canonical expansion
  $ josh-filter ':#/sub1' refs/josh/treeref --update refs/josh/roundtrip_sugar 1> /dev/null
  $ git show refs/josh/roundtrip_sugar:file1
  contents1

ObjectRef: stores the blob OID in a gitlink at path
  $ josh-filter -s ':&sub1/file1' master --update refs/josh/blobref 1> /dev/null
  $ git ls-tree refs/josh/blobref sub1/file1 | cut -d' ' -f1-2
  160000 commit
  $ [ "$(git rev-parse refs/josh/blobref:sub1/file1)" = "$(git rev-parse master:sub1/file1)" ] && echo "match"
  match

ObjectDeref: blob OID gitlink restores the blob at path (round-trip)
  $ josh-filter ':#sub1/file1' refs/josh/blobref --update refs/josh/blobroundtrip 1> /dev/null
  $ git show refs/josh/blobroundtrip:sub1/file1
  contents1

ObjectRef: path not found is treated as nop (reference is updated, path absent from output)
  $ josh-filter ':&version.txt' master --update refs/josh/refnop 1> /dev/null
  $ git rev-parse --verify refs/josh/refnop > /dev/null && echo "updated"
  updated
  $ git show refs/josh/refnop:version.txt 2>/dev/null || echo "not present"
  not present
