  $ export TESTTMP=${PWD}
  $ git init -q 1> /dev/null

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

Filter using the named ref as baseline
  $ josh-filter :/sub1 refs/heads/master --update refs/heads/filtered_ref 1>/dev/null

Filter using a plain SHA — must produce the same filtered commit
  $ SHA=$(git rev-parse HEAD)
  $ josh-filter :/sub1 $SHA --update refs/heads/filtered_sha 1>/dev/null

Both filtered refs must point to the same commit
  $ git diff refs/heads/filtered_ref refs/heads/filtered_sha
