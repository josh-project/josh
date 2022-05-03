  $ export TESTTMP=${PWD}

Create two repos, one is submodule of other

  $ mkdir app lib remote-app remote-lib
  $ git -C app init --quiet
  $ git -C lib init --quiet
  $ git -C remote-app init --bare --quiet
  $ git -C remote-lib init --bare --quiet

This repo will be a submodule

  $ cd ${TESTTMP}/lib
  $ git config push.default current
  $ echo test > file1
  $ git add file1
  $ git commit --quiet -m "submodule: test1"
  $ git remote add origin $(pwd)/../remote-lib
  $ git push --quiet origin

This repo will include a submodule

  $ cd ${TESTTMP}/app
  $ git config push.default current
  $ echo test > file1
  $ git add file1
  $ mkdir modules
  $ git submodule add --quiet ../remote-lib modules/lib
  $ git commit --quiet -m "app: add file and submodule"

Include the folder with submodules and .gitmodules file

  $ josh-filter ':[::modules/,::.gitmodules]'
  $ git checkout --quiet FILTERED_HEAD
  $ git switch --quiet -c filtered
  $ git remote add origin $(pwd)/../remote-app
  $ git push origin --quiet

Clone the filtered repo with submodules

  $ cd ${TESTTMP}
  $ git clone --recursive --quiet --branch filtered $(pwd)/remote-app app-clone
  $ cd app-clone
  $ git submodule foreach git log -1 --pretty=oneline
  Entering 'modules/lib'
  13bf819744c2a46f8cf725e8eed46c18fe84d0c4 submodule: test1
  $ git log -1 --pretty=oneline
  e1a71e368a6eca04de01f2a06fdd2c99a615dc6f app: add file and submodule
  $ git ls-files
  .gitmodules
  modules/lib
