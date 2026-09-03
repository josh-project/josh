  $ export TESTTMP=${PWD}
  $ export JOSH_EXPERIMENTAL_FEATURES=1
  $ git config --global protocol.file.allow always

  $ cd ${TESTTMP}
  $ git init -q submodule-repo 1> /dev/null
  $ cd submodule-repo

  $ mkdir -p foo
  $ echo "foo content" > foo/file1.txt
  $ git add foo
  $ git commit -m "add foo with files" 1> /dev/null

  $ mkdir -p bar
  $ echo "bar content" > bar/file2.txt
  $ git add bar
  $ git commit -m "add bar with file" 1> /dev/null

  $ cd ${TESTTMP}
  $ git init -q main-repo 1> /dev/null
  $ cd main-repo

  $ echo "main content" > main.txt
  $ git add main.txt
  $ git commit -m "add main.txt" 1> /dev/null

  $ git submodule add ../submodule-repo libs 2> /dev/null
  $ git commit -m "add libs submodule" 1> /dev/null
  $ git fetch ../submodule-repo
  From ../submodule-repo
   * branch            HEAD       -> FETCH_HEAD

Dereferencing a gitlink that points at a commit restores the commit's tree and
merges its history at the pointer update.

  $ josh-filter ':#libs' master --update refs/josh/filter/master 1> /dev/null

  $ git log --graph --pretty=%s refs/josh/filter/master
  *   add libs submodule
  |\  
  | * add bar with file
  | * add foo with files
  * add main.txt

The dereference filter selects only the referenced path. The gitlink becomes a
plain tree, while unrelated files and .gitmodules are absent.

  $ git-tree-pretty refs/josh/filter/master
  .
  └── libs/
      ├── bar/
      │   └── file2.txt
      │       ┆  bar content
      └── foo/
          └── file1.txt
              ┆  foo content

Composing the dereference with the original tree minus the referenced path
keeps the main repository content and replaces only the gitlink.

  $ josh-filter ':[:exclude[:#libs],:#libs]' master --update refs/josh/filter/combined 1> /dev/null

The same history splice applies when the dereference is nested in a compose.

  $ [ "$(git rev-list --parents -n 1 refs/josh/filter/combined | wc -w | tr -d ' ')" = 3 ] && echo "submodule history parent present"
  submodule history parent present

  $ git-tree-pretty refs/josh/filter/combined
  .
  ├── .gitmodules
  │   ┆  [submodule "libs"]
  │   ┆  	path = libs
  │   ┆  	url = ../submodule-repo
  ├── libs/
  │   ├── bar/
  │   │   └── file2.txt
  │   │       ┆  bar content
  │   └── foo/
  │       └── file1.txt
  │           ┆  foo content
  └── main.txt
      ┆  main content

Multiple references each contribute a history parent, including duplicate
target commits that land at different prefixed destinations deep in the AST.

  $ cd ${TESTTMP}/submodule-repo
  $ git checkout -b vendor HEAD~1 1> /dev/null 2> /dev/null
  $ mkdir vendor
  $ echo "vendor content" > vendor/file.txt
  $ git add vendor/file.txt
  $ git commit -m "add vendor content" 1> /dev/null
  $ VENDOR_COMMIT=$(git rev-parse HEAD)
  $ LIBS_COMMIT=$(git rev-parse master)
  $ git checkout master 1> /dev/null 2> /dev/null

  $ cd ${TESTTMP}/main-repo
  $ git checkout -b multiple master~1 1> /dev/null 2> /dev/null
  $ git fetch ../submodule-repo master vendor 1> /dev/null 2> /dev/null
  $ git update-index --add --cacheinfo "160000,$LIBS_COMMIT,libs"
  $ git update-index --add --cacheinfo "160000,$LIBS_COMMIT,libs-copy"
  $ git update-index --add --cacheinfo "160000,$VENDOR_COMMIT,vendor"
  $ git commit -m "add three submodule references" 1> /dev/null

  $ josh-filter ':[:#libs:prefix=left,:[:#libs-copy:prefix=right,:#vendor]]' multiple --update refs/josh/filter/multiple 1> /dev/null
  $ [ "$(git rev-list --parents -n 1 refs/josh/filter/multiple | wc -w | tr -d ' ')" = 5 ] && echo "three landing-path history parents present"
  three landing-path history parents present

  $ git ls-tree -r --name-only refs/josh/filter/multiple^2
  left/libs/bar/file2.txt
  left/libs/foo/file1.txt
  $ git ls-tree -r --name-only refs/josh/filter/multiple^3
  right/libs-copy/bar/file2.txt
  right/libs-copy/foo/file1.txt
  $ git ls-tree -r --name-only refs/josh/filter/multiple^4
  vendor/foo/file1.txt
  vendor/vendor/file.txt

  $ git-tree-pretty refs/josh/filter/multiple
  .
  ├── left/
  │   └── libs/
  │       ├── bar/
  │       │   └── file2.txt
  │       │       ┆  bar content
  │       └── foo/
  │           └── file1.txt
  │               ┆  foo content
  ├── right/
  │   └── libs-copy/
  │       ├── bar/
  │       │   └── file2.txt
  │       │       ┆  bar content
  │       └── foo/
  │           └── file1.txt
  │               ┆  foo content
  └── vendor/
      ├── foo/
      │   └── file1.txt
      │       ┆  foo content
      └── vendor/
          └── file.txt
              ┆  vendor content

Updating all three pointers in one superproject commit produces one merge
parent per landing path. The two duplicate targets remain separate histories.

  $ FILTERED_BASE=$(git rev-parse refs/josh/filter/multiple)
  $ cd ${TESTTMP}/submodule-repo
  $ git checkout -b multiple-libs "$LIBS_COMMIT" 1> /dev/null 2> /dev/null
  $ echo "updated libs content" > bar/updated.txt
  $ git add bar/updated.txt
  $ git commit -m "update libs content" 1> /dev/null
  $ UPDATED_LIBS_COMMIT=$(git rev-parse HEAD)
  $ git checkout vendor 1> /dev/null 2> /dev/null
  $ echo "updated vendor content" > vendor/updated.txt
  $ git add vendor/updated.txt
  $ git commit -m "update vendor content" 1> /dev/null
  $ UPDATED_VENDOR_COMMIT=$(git rev-parse HEAD)
  $ git checkout master 1> /dev/null 2> /dev/null

  $ cd ${TESTTMP}/main-repo
  $ git fetch ../submodule-repo multiple-libs vendor 1> /dev/null 2> /dev/null
  $ git update-index --cacheinfo "160000,$UPDATED_LIBS_COMMIT,libs"
  $ git update-index --cacheinfo "160000,$UPDATED_LIBS_COMMIT,libs-copy"
  $ git update-index --cacheinfo "160000,$UPDATED_VENDOR_COMMIT,vendor"
  $ git commit -m "update three submodule references" 1> /dev/null
  $ josh-filter ':[:#libs:prefix=left,:[:#libs-copy:prefix=right,:#vendor]]' multiple --update refs/josh/filter/multiple 1> /dev/null

  $ git log --graph --pretty=%s "${FILTERED_BASE}..refs/josh/filter/multiple" | sed 's/ *$//'
  *-.   update three submodule references
  |\ \
  | | * update vendor content
  | * update libs content
  * update libs content

  $ git checkout master 1> /dev/null 2> /dev/null

Moving the gitlink merges only the newly referenced submodule commits.

  $ cd ${TESTTMP}/submodule-repo
  $ echo "new content" > foo/file3.txt
  $ git add foo/file3.txt
  $ git commit -m "add file3.txt" 1> /dev/null

  $ echo "another new content" > bar/file4.txt
  $ git add bar/file4.txt
  $ git commit -m "add file4.txt" 1> /dev/null

  $ cd ${TESTTMP}/main-repo
  $ git submodule update --remote libs 1> /dev/null 2> /dev/null
  $ git add libs
  $ git commit -m "update libs submodule" 1> /dev/null
  $ git fetch ../submodule-repo
  From ../submodule-repo
   * branch            HEAD       -> FETCH_HEAD

  $ josh-filter ':#libs' master --update refs/josh/filter/master 1> /dev/null

  $ git log --graph --pretty=%s refs/josh/filter/master
  *   update libs submodule
  |\  
  | * add file4.txt
  | * add file3.txt
  |/  
  *   add libs submodule
  |\  
  | * add bar with file
  | * add foo with files
  * add main.txt

  $ git-tree-pretty refs/josh/filter/master
  .
  └── libs/
      ├── bar/
      │   ├── file2.txt
      │   │   ┆  bar content
      │   └── file4.txt
      │       ┆  another new content
      └── foo/
          ├── file1.txt
          │   ┆  foo content
          └── file3.txt
              ┆  new content

Changes made after the submodule was inlined are exported as a new submodule
history. Export leaves the superproject unchanged; updating its gitlink is a
separate change.

The original submodule tip is a trivial merge. Export must retain it while
removing only the superproject pointer-update merges introduced by ObjectDeref.

  $ cd ${TESTTMP}/submodule-repo
  $ git checkout -b trivial-side 1> /dev/null 2> /dev/null
  $ git commit --allow-empty -m "trivial side" 1> /dev/null
  $ git checkout master 1> /dev/null 2> /dev/null
  $ git commit --allow-empty -m "trivial main" 1> /dev/null
  $ git merge --no-ff trivial-side -m "preserved trivial merge" 1> /dev/null
  $ PRESERVED_MERGE=$(git rev-parse HEAD)
  $ [ "$(git rev-list --parents -n 1 HEAD | wc -w | tr -d ' ')" = 3 ] && [ "$(git rev-parse HEAD^{tree})" = "$(git rev-parse HEAD^1^{tree})" ] && echo "trivial merge created"
  trivial merge created

  $ cd ${TESTTMP}/main-repo
  $ git submodule update --remote libs 1> /dev/null 2> /dev/null
  $ git add libs
  $ git commit -m "update libs to trivial merge" 1> /dev/null
  $ git fetch ../submodule-repo 1> /dev/null 2> /dev/null
  $ josh-filter ':#libs' master --update refs/josh/filter/master 1> /dev/null

  $ ORIGINAL_SUBMODULE_TIP=$(git rev-parse master:libs)
  $ git worktree add -b inlined ../inlined refs/josh/filter/master 1> /dev/null 2> /dev/null
  $ cd ../inlined
  $ echo "edited after inline" >> libs/foo/file1.txt
  $ git add libs/foo/file1.txt
  $ git commit -m "edit inlined submodule" 1> /dev/null

  $ josh-filter ':/libs:export' inlined --update refs/heads/exported 1> /dev/null
  $ EXTRACTED_TIP=$(git rev-parse exported)
  $ git merge-base --is-ancestor "${ORIGINAL_SUBMODULE_TIP}" "${EXTRACTED_TIP}" && echo "fast-forward"
  fast-forward
  $ [ "$(git rev-parse exported^)" = "${ORIGINAL_SUBMODULE_TIP}" ] && echo "direct child"
  direct child

  $ cd ../main-repo
  $ [ "$(git rev-parse master:libs)" = "${ORIGINAL_SUBMODULE_TIP}" ] && echo "superproject unchanged"
  superproject unchanged
  $ git --git-dir=../submodule-repo/.git update-ref refs/heads/extracted "${ORIGINAL_SUBMODULE_TIP}"
  $ git push ../submodule-repo "${EXTRACTED_TIP}:refs/heads/extracted" 1> /dev/null 2> /dev/null
  $ git --git-dir=../submodule-repo/.git log --pretty=%s "${ORIGINAL_SUBMODULE_TIP}..extracted"
  edit inlined submodule
  $ git --git-dir=../submodule-repo/.git show -s --pretty=%s "${PRESERVED_MERGE}"
  preserved trivial merge
  $ git --git-dir=../submodule-repo/.git show extracted:foo/file1.txt
  foo content
  edited after inline
