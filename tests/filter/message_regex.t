  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q testrepo 1> /dev/null
  $ cd testrepo

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "fix: add feature" 1> /dev/null

  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "feat: new feature" 1> /dev/null

  $ echo contents3 > file3
  $ git add file3
  $ git commit -m "docs: update documentation" 1> /dev/null

Test that message rewriting with regex works
  $ josh-filter ':"[{type}] {message}";"(?s)^(?P<type>fix|feat|docs): (?P<message>.+)$"' --update refs/josh/filter/master master
  e6cd4a53ce0664f06bbcdd2f5727c114fb4cda7c
  $ git log --pretty=%s josh/filter/master
  [docs] update documentation
  [feat] new feature
  [fix] add feature
  $ git log --pretty=%s master
  docs: update documentation
  feat: new feature
  fix: add feature

  $ cd ${TESTTMP}
  $ git init -q testrepo2 1> /dev/null
  $ cd testrepo2

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "Original commit message" 1> /dev/null

Test that message rewriting with regex and template variables works
  $ josh-filter ':"[{type}] {message} (commit: {commit})";"(?s)^(?P<type>Original) (?P<message>.+)$"' --update refs/josh/filter/master master
  7f14701ff3a86f0e511cfd76d41715cac7dc7999
  $ git log --pretty=%s josh/filter/master
  [Original] commit message  (commit: 16421eebc58313502a347bc92349cc2f52d58fbd)
  $ git cat-file commit josh/filter/master | grep -A 1 "^$"
  
  [Original] commit message

  $ cd ${TESTTMP}
  $ git init -q testrepo3 1> /dev/null
  $ cd testrepo3

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "Subject line with TODO" -m "Body line 1 with TODO" -m "Body line 2" -m "Body line 3 with TODO" 1> /dev/null

Test that message rewriting can remove multiple occurrences from a message with body
  $ josh-filter ':"";"TODO"' --update refs/josh/filter/master master
  5609433160403649c1663beec7d714ea9ee2bb1d
  $ git log -1 --pretty=format:"%B" josh/filter/master | cat
  Subject line with 
  
  Body line 1 with 
  
  Body line 2
  
  Body line 3 with 
  $ git log -1 --pretty=format:"%B" master | cat
  Subject line with TODO
  
  Body line 1 with TODO
  
  Body line 2
  
  Body line 3 with TODO

