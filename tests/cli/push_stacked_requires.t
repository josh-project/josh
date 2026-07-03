Setup

  $ export TESTTMP=${PWD}

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
  $ git add .
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

Clone with josh filter

  $ josh clone ${TESTTMP}/remote :/sub1 filtered
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/filtered
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/filtered/






  $ cd filtered

Set up git config for author

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Create a stack where gamma is textually independent of alpha but shares a
Change-Series, and beta has no series at all.

  $ echo "aaa" > fileA
  $ git add fileA
  $ printf "Change: alpha\n\nChange-Series: dep1" | git commit -q -F -

  $ echo "bbb" > fileB
  $ git add fileB
  $ git commit -q -m "Change: beta"

  $ echo "ccc" > fileC
  $ git add fileC
  $ printf "Change: gamma\n\nChange-Series: dep1" | git commit -q -F -

  $ git log --decorate --graph --pretty="%s %d"
  * Change: gamma  (HEAD -> master)
  * Change: beta 
  * Change: alpha 
  * add file1  (origin/master, origin/HEAD)

Publish with split mode

  $ josh changes publish > /dev/null 2>&1

Verify gamma's downstack includes alpha (due to shared Change-Series: dep1)
but not beta (different series / no series, no file overlap).

  $ cd ${TESTTMP}/remote

  $ git log refs/heads/@changes/master/josh@example.com/gamma --pretty="%s"
  Change: gamma
  Change: alpha
  add file1

Alpha stands alone on base (no dependencies)

  $ git log refs/heads/@changes/master/josh@example.com/alpha --pretty="%s"
  Change: alpha
  add file1

Beta also stands alone on base (no dependencies)

  $ git log refs/heads/@changes/master/josh@example.com/beta --pretty="%s"
  Change: beta
  add file1
