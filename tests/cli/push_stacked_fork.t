Publishing from a fork: change branches are pushed to the fork (push-url),
while the upstream repo receives nothing.

  $ export TESTTMP=${PWD}

Create an upstream bare repo with initial content

  $ git init -q --bare upstream
  $ git init -q seed
  $ cd seed
  $ mkdir -p sub1
  $ echo "file1 content" > sub1/file1
  $ git add .
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/upstream
  $ git push -q origin master
  $ cd ..

Create an empty bare fork to receive the pushed change branches

  $ git init -q --bare fork

Clone with a filter and a separate push-url pointing at the fork

  $ josh clone ${TESTTMP}/upstream :/sub1 filtered --push-url ${TESTTMP}/fork
  Added remote 'origin' with filter ':/sub1'
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/filtered/
  $ cd filtered

Make two stacked changes

  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "Change-Id: 1234"
  $ echo "contents3" > file3
  $ git add file3
  $ git commit -q -m "Change-Id: foo7"
  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Publish the stack; branches are routed to the fork

  $ josh changes publish
  published 2 changes (2 new)

The upstream repo must NOT have received any change refs (only master)

  $ git ls-remote ${TESTTMP}/upstream
  115b269a011d493259a125fa941fd790b903175f\tHEAD (esc)
  115b269a011d493259a125fa941fd790b903175f\trefs/heads/master (esc)

The fork must have received the @changes / @base / @heads refs

  $ git ls-remote ${TESTTMP}/fork
  115b269a011d493259a125fa941fd790b903175f\trefs/heads/@base/master/josh@example.com/1234 (esc)
  115b269a011d493259a125fa941fd790b903175f\trefs/heads/@base/master/josh@example.com/foo7 (esc)
  478c423aa5b34433bcb04513deb8f788958099c1\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  225aa057b057ef4fafbc2cd6916680e176dc4152\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  39844a5762c65ef897eb0d7efcfa446d5d99fcea\trefs/heads/@heads/master/josh@example.com (esc)
