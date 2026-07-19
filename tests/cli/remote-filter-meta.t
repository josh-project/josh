Semantic meta args (history=..., gpgsig=...) in a remote's filter must be applied
when filtering, not stripped together with the transport keys (url, fetch, forge)
that the remote config stores in the same meta block.

  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ mkdir remote
  $ cd remote
  $ git init -q libs 1> /dev/null
  $ cd libs

The push roundtrip below pushes to this non-bare repo's checked-out branch

  $ git config receive.denyCurrentBranch ignore

Base commit inside the filtered subtree

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

A feature branch that only touches files OUTSIDE sub1/ ...

  $ git checkout -q -b branch1
  $ echo outside > outside_file
  $ git add outside_file
  $ git commit -m "add outside_file" 1> /dev/null

... merged into a master that also advanced inside sub1/. The merge is
trivial under :/sub1 (does not change the filtered tree), so it is elided by
default and kept with history="keep-trivial-merges".

  $ git checkout -q master
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ git merge -q branch1 --no-ff
  $ git log --graph --pretty=%s
  *   Merge branch 'branch1'
  |\  
  | * add outside_file
  * | add file2
  |/  
  * add file1

  $ cd ${TESTTMP}

Clone with the meta args: the kept merge must show up in the filtered history

  $ josh clone ${TESTTMP}/remote/libs ':~(history="keep-trivial-merges",gpgsig="norm-lf")[:/sub1]' libs
  Added remote 'origin' with filter ':~(history="keep-trivial-merges",gpgsig="norm-lf")[:/sub1]'
  From file://${TESTTMP}/remote/libs
   * [new branch]      branch1    -> refs/josh/remotes/origin/branch1
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs
   * [new branch]      branch1    -> origin/branch1
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/libs/

  $ cd libs
  $ git log --graph --pretty=%s origin/master
  *   Merge branch 'branch1'
  |\  
  * | add file2
  |/  
  * add file1

Equivalence with josh-filter: the CLI's filtered head must be SHA-identical to
what josh-filter produces for the same spec in the source repo

  $ cd ${TESTTMP}/remote/libs
  $ josh-filter -s ':~(history="keep-trivial-merges",gpgsig="norm-lf")[:/sub1]' master --update refs/heads/expected 1> /dev/null
  $ git rev-parse expected
  2fa5077e262980fdd1efebd9093f1be9b9a2192c

  $ cd ${TESTTMP}/libs
  $ git rev-parse origin/master
  2fa5077e262980fdd1efebd9093f1be9b9a2192c

Counter-check: without the meta args the merge is elided (proves the setup
distinguishes the two and the args do not leak into the plain path)

  $ cd ${TESTTMP}
  $ josh clone ${TESTTMP}/remote/libs ':/sub1' libs-plain
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote/libs
   * [new branch]      branch1    -> refs/josh/remotes/origin/branch1
   * [new branch]      expected   -> refs/josh/remotes/origin/expected
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs-plain
   * [new branch]      branch1    -> origin/branch1
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/libs-plain/

  $ cd libs-plain
  $ git log --graph --pretty=%s origin/master
  * add file2
  * add file1

Incremental fetch: another trivial-under-filter merge upstream must also be kept

  $ cd ${TESTTMP}/remote/libs
  $ git checkout -q -b branch2
  $ echo outside2 > outside_file2
  $ git add outside_file2
  $ git commit -m "add outside_file2" 1> /dev/null
  $ git checkout -q master
  $ echo contents3 > sub1/file3
  $ git add sub1
  $ git commit -m "add file3" 1> /dev/null
  $ git merge -q branch2 --no-ff

  $ cd ${TESTTMP}/libs
  $ josh fetch
  From file://${TESTTMP}/remote/libs
   * [new branch]      branch2    -> refs/josh/remotes/origin/branch2
   * [new branch]      expected   -> refs/josh/remotes/origin/expected
     0823744..5c2a751  master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs
     2fa5077..c2a3e28  master     -> origin/master
   * [new branch]      branch2    -> origin/branch2
  
  Fetched from remote: origin

  $ git log --graph --pretty=%s origin/master
  *   Merge branch 'branch2'
  |\  
  * | add file3
  |/  
  *   Merge branch 'branch1'
  |\  
  * | add file2
  |/  
  * add file1

josh filter prints the spec including the semantic args

  $ josh filter origin
  Applying filter ':~(gpgsig="norm-lf",history="keep-trivial-merges")[:/sub1]' to remote 'origin'
  Applied filter ':~(gpgsig="norm-lf",history="keep-trivial-merges")[:/sub1]' to remote 'origin'

Push roundtrip: reverse filtering must use the same semantic filter

  $ git checkout -q -b work origin/master
  $ echo pushed > pushed_file
  $ git add pushed_file
  $ git commit -m "add pushed_file" 1> /dev/null
  $ josh push origin work:refs/heads/master
  Pushing eb7746f155f6de988eaa9727a78c4449017e23b3 to origin/refs/heads/master
  To file://${TESTTMP}/remote/libs
     5c2a751..eb7746f  eb7746f155f6de988eaa9727a78c4449017e23b3 -> master
  
  Pushed 1 ref(s) to origin

  $ cd ${TESTTMP}/remote/libs
  $ git log --pretty=%s -1 master
  add pushed_file
  $ git show --pretty=%s --stat master
  add pushed_file
  
   sub1/pushed_file | 1 +
   1 file changed, 1 insertion(+)

A fetch after the push is a fast-forward onto the pushed commit

  $ cd ${TESTTMP}/libs
  $ josh fetch
  From file://${TESTTMP}/remote/libs
     5c2a751..eb7746f  master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs
     c2a3e28..7c8cc20  master     -> origin/master
  
  Fetched from remote: origin
  $ git log --pretty=%s -1 origin/master
  add pushed_file

Chain filter with meta args: stepwise application by the CLI must equal the
composed application josh-filter performs

  $ cd ${TESTTMP}
  $ josh clone ${TESTTMP}/remote/libs ':~(history="keep-trivial-merges")[:/sub1:prefix=libs]' libs-chain
  Added remote 'origin' with filter ':~(history="keep-trivial-merges")[:/sub1:prefix=libs]'
  From file://${TESTTMP}/remote/libs
   * [new branch]      branch1    -> refs/josh/remotes/origin/branch1
   * [new branch]      branch2    -> refs/josh/remotes/origin/branch2
   * [new branch]      expected   -> refs/josh/remotes/origin/expected
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/libs-chain
   * [new branch]      branch1    -> origin/branch1
   * [new branch]      branch2    -> origin/branch2
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/libs-chain/

  $ cd ${TESTTMP}/remote/libs
  $ josh-filter -s ':~(history="keep-trivial-merges")[:/sub1:prefix=libs]' master --update refs/heads/expected-chain 1> /dev/null
  $ git rev-parse expected-chain
  963f9f17ffd626f046558444e2474f1b223bc272

  $ cd ${TESTTMP}/libs-chain
  $ git rev-parse origin/master
  963f9f17ffd626f046558444e2474f1b223bc272

Reserved transport keys are rejected in user filters

  $ cd ${TESTTMP}
  $ josh clone ${TESTTMP}/remote/libs ':~(url="ha")[:/sub1]' libs-bad
  Error: Failed to write remote config file
  Failed to write remote config file
  Filter must not set reserved meta key 'url': it is owned by the remote config
  [1]
