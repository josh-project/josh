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

Roundtrip: no subfilter omits brackets
  $ josh-filter -p ':#version.txt'
  :#version.txt

Apply via stored filter: blob at path contains tree OID of subfilter result
  $ cat > filter.josh <<'EOF'
  > :#version.txt[:/sub1]
  > EOF
  $ git add filter.josh
  $ git commit -m "add filter" 1> /dev/null
  $ josh-filter -s :+filter master --update refs/josh/master 1> /dev/null
  $ [ "$(git show refs/josh/master:version.txt)" = "$(git rev-parse master:sub1)" ] && echo "match"
  match

Apply: no subfilter uses empty tree OID
  $ cat > filter2.josh <<'EOF'
  > :#version.txt
  > EOF
  $ git add filter2.josh
  $ git commit -m "add filter2" 1> /dev/null
  $ josh-filter -s :+filter2 master --update refs/josh/empty 1> /dev/null
  $ EMPTY_TREE=$(git hash-object -t tree /dev/null)
  $ [ "$(git show refs/josh/empty:version.txt)" = "${EMPTY_TREE}" ] && echo "match"
  match
