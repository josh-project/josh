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
