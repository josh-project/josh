Disjoint remote mounts compose in a full worktree, while each push extracts only
the selected remote's mount.

  $ export TESTTMP=${PWD}

  $ git init -q --bare central
  $ git init -q central-seed
  $ mkdir -p central-seed/app
  $ echo "central v1" > central-seed/app/main.txt
  $ git -C central-seed add app
  $ git -C central-seed commit -q -m "central initial"
  $ git -C central-seed remote add origin ${TESTTMP}/central
  $ git -C central-seed push -q origin master

  $ git init -q --bare vendor
  $ git init -q vendor-seed
  $ echo "vendor v1" > vendor-seed/lib.txt
  $ git -C vendor-seed add lib.txt
  $ git -C vendor-seed commit -q -m "vendor initial"
  $ git -C vendor-seed remote add origin ${TESTTMP}/vendor
  $ git -C vendor-seed push -q origin master

  $ josh init store >/dev/null
  $ cd store
  $ josh remote add central ${TESTTMP}/central 2>/dev/null
  $ josh remote add libs/vendored ${TESTTMP}/vendor 2>/dev/null
  $ josh worktree add full 2>/dev/null >/dev/null
  $ cd full
  $ git merge -q --allow-unrelated-histories --no-edit libs%2Fvendored/master
  $ git-tree-pretty HEAD
  .
  ├── central/
  │   └── app/
  │       └── main.txt
  │           ┆  central v1
  └── libs/
      └── vendored/
          └── lib.txt
              ┆  vendor v1

  $ echo "central v2" >> central/app/main.txt
  $ echo "vendor v2" >> libs/vendored/lib.txt
  $ git add .
  $ git commit -q -m "update both mounts"
  $ josh push libs%2Fvendored HEAD:refs/heads/master >/dev/null 2>&1
  $ josh push central HEAD:refs/heads/master >/dev/null 2>&1

  $ git --git-dir=${TESTTMP}/vendor show master:lib.txt
  vendor v1
  vendor v2
  $ (cd ${TESTTMP}/vendor && git-tree-pretty master)
  .
  └── lib.txt
      ┆  vendor v1
      ┆  vendor v2
  $ git --git-dir=${TESTTMP}/central show master:app/main.txt
  central v1
  central v2
  $ (cd ${TESTTMP}/central && git-tree-pretty master)
  .
  └── app/
      └── main.txt
          ┆  central v1
          ┆  central v2
