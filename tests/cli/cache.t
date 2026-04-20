  $ export TESTTMP=${PWD}


  $ cd ${TESTTMP}

Setup: create a bare remote and populate it with some commits

  $ mkdir remote
  $ cd remote
  $ git init -q --bare libs 1>/dev/null
  $ cd ..

  $ mkdir source
  $ cd source
  $ git init -q 1>/dev/null
  $ mkdir sub1
  $ echo file1 > sub1/file1
  $ echo file2 > sub1/file2
  $ git add sub1
  $ git commit -q -m "add files"
  $ git remote add origin ${TESTTMP}/remote/libs
  $ git push -q origin master
  $ cd ..

  $ which git
  /opt/git-install/bin/git

Initialize a local workspace and add the josh remote (no fetch yet)

  $ git init -q local1 1>/dev/null
  $ cd local1
  $ josh remote add origin ${TESTTMP}/remote/libs :/sub1
  Added remote 'origin' with filter ':/sub1'

Build the distributed cache (applies the filter to already-fetched refs)

  $ josh fetch
  From file://${TESTTMP}/remote/libs
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/local1
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin

  $ josh cache build
  Built cache for 1 filter(s) on branch 'master' for remote 'origin'


Verify local cache refs were created

  $ git for-each-ref --format='%(refname)' 'refs/josh/cache/'
  refs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d

Push the distributed cache and filtered ref to the backing remote

  $ josh cache push
  To file://${TESTTMP}/remote/libs
   * [new reference]   refs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d -> refs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d
  
  To file://${TESTTMP}/remote/libs
   * [new reference]   refs/josh/filtered/bf567e0faf634a663d6cef48145a035e1974ab1d/heads/master -> refs/josh/filtered/bf567e0faf634a663d6cef48145a035e1974ab1d/heads/master
  
  Pushed cache for remote 'origin' (filter: bf567e0f)

Verify the remote now has cache refs and filtered refs

  $ git ls-remote ${TESTTMP}/remote/libs 'refs/josh/cache/*' | wc -l | tr -d ' '
  1

  $ git ls-remote ${TESTTMP}/remote/libs 'refs/josh/filtered/*' | wc -l | tr -d ' '
  1

  $ cd ..

Initialize a second local workspace to test cache fetch

  $ git init -q local2 1>/dev/null
  $ cd local2
  $ josh remote add origin ${TESTTMP}/remote/libs :/sub1
  Added remote 'origin' with filter ':/sub1'

Verify no cache refs before fetch

  $ git for-each-ref --format='%(refname)' 'refs/josh/cache/' | wc -l | tr -d ' '
  0

Fetch the distributed cache and filtered objects from the remote

  $ josh cache fetch
  From file://${TESTTMP}/remote/libs
   * [new ref]         refs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d -> refs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d
  
  From file://${TESTTMP}/remote/libs
   * [new ref]         refs/josh/filtered/bf567e0faf634a663d6cef48145a035e1974ab1d/heads/master -> refs/josh/filtered/bf567e0faf634a663d6cef48145a035e1974ab1d/heads/master
  
  Fetched cache for remote 'origin' (filter: bf567e0f)

Verify cache refs are now present locally

  $ git for-each-ref --format='%(refname)' 'refs/josh/cache/'
  refs/josh/cache/26/0/bf567e0faf634a663d6cef48145a035e1974ab1d

  $ cd ${TESTTMP}

Chain filter section: test that intermediate refs are created for each step in the chain

  $ git init -q local3 1>/dev/null
  $ cd local3
  $ josh remote add origin ${TESTTMP}/remote/libs :/sub1:prefix=libs
  Added remote 'origin' with filter ':/sub1:prefix=libs'

  $ josh fetch 2>/dev/null

  $ josh cache build
  Built cache for 1 filter(s) on branch 'master' for remote 'origin'

After building a 2-step chain, there should be 2 filtered ref entries locally (one per step)

  $ git for-each-ref --format='%(refname)' 'refs/josh/filtered/' | wc -l | tr -d ' '
  2

Push cache and both step refs to the remote

  $ josh cache push 2>/dev/null

Remote should now have 2 filtered ref entries (step-0 and step-1)

  $ git ls-remote ${TESTTMP}/remote/libs 'refs/josh/filtered/*' | wc -l | tr -d ' '
  2

  $ cd ${TESTTMP}

Test cache fetch in a fresh repo with a chain filter

  $ git init -q local4 1>/dev/null
  $ cd local4
  $ josh remote add origin ${TESTTMP}/remote/libs :/sub1:prefix=libs
  Added remote 'origin' with filter ':/sub1:prefix=libs'

  $ git for-each-ref --format='%(refname)' 'refs/josh/filtered/' | wc -l | tr -d ' '
  0

  $ josh cache fetch 2>/dev/null

After fetching, both sets of intermediate refs should be present locally

  $ git for-each-ref --format='%(refname)' 'refs/josh/filtered/' | wc -l | tr -d ' '
  2

  $ cd ${TESTTMP}
