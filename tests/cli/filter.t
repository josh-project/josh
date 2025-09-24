  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ mkdir remote
  $ cd remote
  $ git init -q libs 1> /dev/null
  $ cd libs

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add files" 1> /dev/null

  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

Test josh filter command - apply filtering without fetching

  $ josh clone ${TESTTMP}/remote/libs:/sub1 filtered-repo
  Successfully added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  Successfully fetched from remote: origin
  Successfully pulled from remote: origin
  Successfully cloned repository to: filtered-repo

  $ cd filtered-repo

  $ ls
  file1
  file2

  $ git log --oneline
  1432d42 add files

  $ git config remote.origin.josh-filter
  :/sub1:prune=trivial-merge

  $ josh filter origin
  Applying filter ':/sub1:prune=trivial-merge' to remote 'origin'
  Successfully applied filter to remote: origin

  $ git log --oneline
  1432d42 add files

  $ cd ..

Test josh filter with non-existent remote

  $ mkdir test-repo
  $ cd test-repo
  $ git init -q

  $ josh filter nonexistent
  Error: No filter configured for remote 'nonexistent': config value 'remote.nonexistent.josh-filter' was not found; class=Config (7); code=NotFound (-3)
  [1]

  $ cd ..


