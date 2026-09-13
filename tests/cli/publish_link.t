  $ export TESTTMP=${PWD}
  $ export JOSH_EXPERIMENTAL_FEATURES=1

Create the monorepo upstream and a linked remote.

  $ mkdir upstream
  $ cd upstream
  $ git init -q --bare
  $ mkdir ${TESTTMP}/lib-repo
  $ git init -q --bare ${TESTTMP}/lib-repo
  $ cd ${TESTTMP}

  $ mkdir work
  $ cd work
  $ git init -q
  $ mkdir -p app lib
  $ echo "app v1" > app/app.txt
  $ echo "lib v1" > lib/lib.txt
  $ git add .
  $ git commit -q -m "initial"
  $ git remote add origin ${TESTTMP}/upstream
  $ git push -q origin master
  $ cd ${TESTTMP}

Clone the monorepo and link the lib/ view to its own remote.

  $ josh clone ${TESTTMP}/upstream :/ mono | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added remote 'origin' with filter ':/'
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/mono/
  $ cd mono

  $ josh link add ${TESTTMP}/lib-repo :/lib --id lib | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added link 'lib': ${TESTTMP}/lib-repo refs/heads/master :/lib

Make a change touching both app/ and lib/, and one touching only app/.

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

  $ echo "app v2" > app/app.txt
  $ echo "lib v2" > lib/lib.txt
  $ git add .
  $ git commit -q -m "change both" -m "Change-Id: both1"
  $ echo "app v3" > app/app.txt
  $ git add .
  $ git commit -q -m "app only" -m "Change-Id: apponly1"

Publishing requires the experimental flag while links exist, and fails before
touching any remote.

  $ env -u JOSH_EXPERIMENTAL_FEATURES josh changes publish
  Error: josh link requires JOSH_EXPERIMENTAL_FEATURES=1
  josh link requires JOSH_EXPERIMENTAL_FEATURES=1
  [1]
  $ git ls-remote ${TESTTMP}/upstream
  *\tHEAD (esc) (glob)
  *\trefs/heads/master (esc) (glob)

Publish: both changes go to the monorepo, only the lib/-touching one goes to
the linked remote.

  $ josh changes publish 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  published 2 changes (2 new)
  Publishing to link 'lib' (${TESTTMP}/lib-repo):
  published 1 change (1 new)

  $ git ls-remote ${TESTTMP}/upstream
  *\tHEAD (esc) (glob)
  *\trefs/heads/@base/master/josh@example.com/apponly1 (esc) (glob)
  *\trefs/heads/@base/master/josh@example.com/both1 (esc) (glob)
  *\trefs/heads/@changes/master/josh@example.com/apponly1 (esc) (glob)
  *\trefs/heads/@changes/master/josh@example.com/both1 (esc) (glob)
  *\trefs/heads/@heads/master/josh@example.com (esc) (glob)
  *\trefs/heads/master (esc) (glob)

  $ git ls-remote ${TESTTMP}/lib-repo
  *\trefs/heads/@base/master/josh@example.com/both1 (esc) (glob)
  *\trefs/heads/@changes/master/josh@example.com/both1 (esc) (glob)
  *\trefs/heads/@heads/master/josh@example.com (esc) (glob)

The change published to the link contains only the lib/ projection.

  $ git -C ${TESTTMP}/lib-repo ls-tree --name-only refs/heads/@changes/master/josh@example.com/both1
  lib.txt
  $ git -C ${TESTTMP}/lib-repo show refs/heads/@changes/master/josh@example.com/both1:lib.txt
  lib v2

A second publish updates the existing refs; unchanged changes are not
re-published.

  $ echo "lib v3" > lib/lib.txt
  $ git add .
  $ git commit -q -m "lib v3" -m "Change-Id: libonly1"
  $ josh changes publish 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  published 1 change (1 new)
  Publishing to link 'lib' (${TESTTMP}/lib-repo):
  published 1 change (1 new)

  $ git -C ${TESTTMP}/lib-repo show refs/heads/@changes/master/josh@example.com/libonly1:lib.txt
  lib v3
