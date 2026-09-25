  $ export TESTTMP=${PWD}
  $ export JOSH_EXPERIMENTAL_FEATURES=1
  $ git config --global protocol.file.allow always

Create a submodule remote and a superproject that points at it. The submodule
commit is deliberately absent from the superproject object database.

  $ git init -q lib
  $ cd lib
  $ echo "lib v1" > lib.txt
  $ git add lib.txt
  $ git commit -q -m "lib initial"
  $ LIB_BASE=$(git rev-parse HEAD)
  $ cd ${TESTTMP}

  $ git init -q --bare super-remote
  $ git init -q super
  $ cd super
  $ git remote add origin ../super-remote
  $ echo "main" > main.txt
  $ git add main.txt
  $ git commit -q -m "main initial"
  $ git submodule add -q --name lib ../lib modules/lib
  $ git commit -q -m "add lib submodule"
  $ git push -q origin master
  $ git cat-file -e ${LIB_BASE}^{commit}
  fatal: Not a valid object name 2aff4866cd849a18c15ca873bdf9602a5a0a30c4^{commit}
  [128]

Adding a combined remote imports configured submodules as export links, fetches
the pointed-to commits, and derives the dereference filter from `.gitmodules`.
Relative submodule URLs are resolved from the superproject URL.

  $ josh remote add combined ../super-remote --submodules --no-forge
  Added submodule link 'lib': ../super-remote/../lib refs/heads/master :/modules/lib:export
  Added remote 'combined' with filter ':~(history="embed")[:[:exclude[:#modules/lib],:#modules/lib]]'
  $ git cat-file -t ${LIB_BASE}
  commit
  $ git rev-parse refs/josh/submodules/lib
  2aff4866cd849a18c15ca873bdf9602a5a0a30c4
  $ josh link list
  lib\t../super-remote/../lib\trefs/heads/master\t\t:/modules/lib:export (escaped)

The imported submodule objects make the gitlink dereference possible, and the
resulting combined history contains the original submodule history as its
second parent.

  $ josh fetch --remote combined
  new branch master
  Fetched from remote: combined
  $ git submodule deinit -q -f --all
  $ git checkout -q -b combined combined/master
  $ git show -s --format=%s HEAD^2
  lib initial

A change made in the combined history publishes to the submodule link. Its
change ref must descend directly from the original submodule tip, not from the
superproject history.

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"
  $ echo "lib v2" > modules/lib/lib.txt
  $ git add modules/lib/lib.txt
  $ git commit -q -m "lib v2" -m "Change-Id: lib2"
  $ josh changes publish combined
  published 1 change (1 new)
  Publishing to link 'lib' (../super-remote/../lib):
  published 1 change (1 new)

  $ git -C ../lib rev-parse refs/heads/@changes/master/josh@example.com/lib2^
  2aff4866cd849a18c15ca873bdf9602a5a0a30c4
  $ git -C ../lib show refs/heads/@changes/master/josh@example.com/lib2:lib.txt
  lib v2
