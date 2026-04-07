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

Apply via stored filter: blob at path contains tree OID of subfilter result
  $ cat > filter.josh <<'EOF'
  > :#version.txt[:/sub1]
  > EOF
  $ git add filter.josh
  $ git commit -m "add filter" 1> /dev/null
  $ josh-filter -s :+filter master --update refs/josh/master 1> /dev/null
  $ [ "$(git show refs/josh/master:version.txt)" = "$(git rev-parse master:sub1)" ] && echo "match"
  match

Deref: path not found is treated as nop (reference is updated, path absent from output)
  $ josh-filter ':#version.txt' master --update refs/josh/noptest 1> /dev/null
  $ git rev-parse --verify refs/josh/noptest > /dev/null && echo "updated"
  updated
  $ git show refs/josh/noptest:version.txt 2>/dev/null || echo "not present"
  not present

Deref: blob containing a valid tree SHA resolves and inserts at path
  $ git rev-parse master:sub1 > ptr.txt
  $ git add ptr.txt
  $ git commit -m "add ptr" 1> /dev/null
  $ josh-filter -s ':#ptr.txt' master --update refs/josh/deref 1> /dev/null
  $ git show refs/josh/deref:ptr.txt/file1
  contents1

Deref: blob with invalid content inserts empty blob at path
  $ printf 'not-a-sha\n' > bad_ptr.txt
  $ git add bad_ptr.txt
  $ git commit -m "add bad_ptr" 1> /dev/null
  $ josh-filter ':#bad_ptr.txt' master --update refs/josh/badtest 1> /dev/null
  $ git rev-parse --verify refs/josh/badtest > /dev/null && echo "updated"
  updated
  $ git show refs/josh/badtest:bad_ptr.txt | wc -c | tr -d ' '
  0

Deref: blob with valid SHA but object not in repo is an error (reference not updated)
  $ printf '0000000000000000000000000000000000000001\n' > ghost_ptr.txt
  $ git add ghost_ptr.txt
  $ git commit -m "add ghost_ptr" 1> /dev/null
  $ josh-filter ':#ghost_ptr.txt' master --update refs/josh/ghosttest 2>&1; echo "exit:$?"
  *:#: object not found in repo: 0000000000000000000000000000000000000001 (glob)
  exit:1
  $ git rev-parse --verify refs/josh/ghosttest 2>/dev/null || echo "not updated"
  not updated

ObjectRef: stores tree OID of sub1 as blob content at sub1
  $ josh-filter -s ':&sub1' master --update refs/josh/treeref 1> /dev/null
  $ [ "$(git show refs/josh/treeref:sub1)" = "$(git rev-parse master:sub1)" ] && echo "match"
  match

ObjectRef + ObjectDeref round-trip: tree entry restored
  $ josh-filter ':#sub1' refs/josh/treeref --update refs/josh/roundtrip 1> /dev/null
  $ git show refs/josh/roundtrip:sub1/file1
  contents1

ObjectRef + :#/path sugar round-trip: tree entry restored via canonical expansion
  $ josh-filter ':#/sub1' refs/josh/treeref --update refs/josh/roundtrip_sugar 1> /dev/null
  $ git show refs/josh/roundtrip_sugar:file1
  contents1

ObjectRef: stores blob OID as blob content at path
  $ josh-filter -s ':&sub1/file1' master --update refs/josh/blobref 1> /dev/null
  $ [ "$(git show refs/josh/blobref:sub1/file1)" = "$(git rev-parse master:sub1/file1)" ] && echo "match"
  match

ObjectDeref: blob OID reference wraps blob at path (round-trip)
  $ josh-filter ':#sub1/file1' refs/josh/blobref --update refs/josh/blobroundtrip 1> /dev/null
  $ git show refs/josh/blobroundtrip:sub1/file1
  contents1

ObjectRef: path not found is treated as nop (reference is updated, path absent from output)
  $ josh-filter ':&version.txt' master --update refs/josh/refnop 1> /dev/null
  $ git rev-parse --verify refs/josh/refnop > /dev/null && echo "updated"
  updated
  $ git show refs/josh/refnop:version.txt 2>/dev/null || echo "not present"
  not present
