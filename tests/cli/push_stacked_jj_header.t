Setup

  $ export TESTTMP=${PWD}

Helper: rewrite HEAD's commit object to add a custom `change-id` header,
mimicking what jj / gitbutler produce. Takes the change-id value as $1.

  $ add_change_id_header() {
  >   orig=$(git rev-parse HEAD)
  >   git cat-file commit "$orig" \
  >     | awk -v cid="$1" '/^$/ && !done {print "change-id " cid; done=1} {print}' \
  >     | git hash-object -t commit -w --stdin > .new.oid
  >   git update-ref HEAD "$(cat .new.oid)"
  >   rm .new.oid
  > }

Create a test repository with some content

  $ mkdir remote
  $ cd remote
  $ git init -q --bare
  $ cd ..

  $ mkdir local
  $ cd local
  $ git init -q
  $ mkdir -p sub1
  $ echo "file1 content" > sub1/file1
  $ echo "before" > file7
  $ git add .
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

Clone with josh filter

  $ josh clone ${TESTTMP}/remote :/sub1 filtered > /dev/null 2>&1
  $ cd filtered

Set up git config for author (must match GIT_AUTHOR_EMAIL used at commit time)

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Commit 1: only a custom `change-id` header — no `Change:` / `Change-Id:` trailer

  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "add file2"
  $ add_change_id_header mlqnqnkrxpuvuuxzlzoltostwlwyskpx

Commit 2: custom header AND a conflicting `Change:` trailer — header must win

  $ echo "contents7" > file7
  $ git add file7
  $ printf 'update file7\n\nChange: footer-should-lose\n' | git commit -q -F -
  $ add_change_id_header qpvuntsmwlqxrkokvyzpswuuxmrlnkqz

Confirm the custom headers landed on the commit objects

  $ git cat-file commit HEAD~ | grep '^change-id '
  change-id mlqnqnkrxpuvuuxzlzoltostwlwyskpx
  $ git cat-file commit HEAD | grep '^change-id '
  change-id qpvuntsmwlqxrkokvyzpswuuxmrlnkqz

Publish the stack — refs must carry the header change-ids, not the footer one

  $ josh changes publish > publish.out 2>&1
  $ grep -o '@changes/master/josh@example.com/[a-z0-9-]*' publish.out | sort -u
  @changes/master/josh@example.com/mlqnqnkrxpuvuuxzlzoltostwlwyskpx
  @changes/master/josh@example.com/qpvuntsmwlqxrkokvyzpswuuxmrlnkqz
  $ grep footer-should-lose publish.out && echo "FAIL: footer leaked into refs" || echo "ok: footer ignored"
  ok: footer ignored

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git for-each-ref --format='%(refname)' 'refs/heads/@changes/master/josh@example.com/*' | sort
  refs/heads/@changes/master/josh@example.com/mlqnqnkrxpuvuuxzlzoltostwlwyskpx
  refs/heads/@changes/master/josh@example.com/qpvuntsmwlqxrkokvyzpswuuxmrlnkqz
