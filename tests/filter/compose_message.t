  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1> /dev/null
  $ cd repo

  $ mkdir -p mono extras
  $ echo a > mono/a.txt
  $ echo b > extras/b.txt
  $ git add .
  $ git commit -m "initial commit" 1> /dev/null

  $ josh-filter -p ':[:/mono,:/extras]:"REWRITTEN"'
  :[
      :/mono
      :/extras
  ]:"REWRITTEN"

  $ josh-filter ':[:/mono,:/extras]:"REWRITTEN"' > /dev/null
  $ git log --pretty=%B -1 FILTERED_HEAD
  REWRITTEN
