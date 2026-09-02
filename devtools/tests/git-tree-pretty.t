  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo
  $ cd repo

  $ mkdir -p src/util
  $ echo "hello world" > src/hello.txt
  $ echo "print('hi')" > src/util/tool.py
  $ printf 'first line\nno newline' > notes.txt
  $ printf '\xffELF\x01\x02' > blob.bin
  $ ln -s hello.txt src/link.txt

  $ git add -A
  $ git commit -qm initial

  $ git-tree-pretty
  .
  ├── blob.bin
  │   ┆  <binary, 6 bytes>
  │   ┆  ff 45 4c 46 01 02                                 ·ELF··
  ├── notes.txt
  │   ┆  first line
  │   ╵  no newline
  └── src/
      ├── hello.txt
      │   ┆  hello world
      ├── link.txt
      │   ╵  hello.txt
      └── util/
          └── tool.py
              ┆  print('hi')

  $ cd ${TESTTMP}
  $ git-tree-pretty --repo repo HEAD --no-contents
  .
  ├── blob.bin
  ├── notes.txt
  └── src/
      ├── hello.txt
      ├── link.txt
      └── util/
          └── tool.py

  $ git-tree-pretty --repo repo nosuchref
  error: failed to resolve "nosuchref": couldn't parse revision: "nosuchref": The ref partially named "nosuchref" could not be found
  [1]

  $ git init -q snapshots
  $ cd snapshots
  $ printf 'ignored.txt\n' > .gitignore
  $ printf 'remove\n' > deleted.txt
  $ printf 'head\n' > staged.txt
  $ git add -A
  $ git commit -qm initial
  $ printf 'index\n' > staged.txt
  $ git add staged.txt
  $ printf 'worktree\n' > staged.txt
  $ rm deleted.txt
  $ printf 'new\n' > untracked.txt
  $ printf 'ignored\n' > ignored.txt

  $ git-tree-pretty +
  .
  ├── .gitignore
  │   ┆  ignored.txt
  ├── deleted.txt
  │   ┆  remove
  └── staged.txt
      ┆  index

  $ git-tree-pretty .
  .
  ├── .gitignore
  │   ┆  ignored.txt
  ├── staged.txt
  │   ┆  worktree
  └── untracked.txt
      ┆  new

  $ cd ${TESTTMP}

  $ git init -q empty
  $ cd empty
  $ touch placeholder
  $ git add placeholder
  $ git commit -qm initial
  $ git rm -q placeholder
  $ git commit -qm "empty tree"
  $ git-tree-pretty HEAD^{tree}
  .

Gitlinks expose their target object kind and optionally expand available targets.
The entry mode remains `commit` even when Josh stores another object kind.

  $ cd ${TESTTMP}
  $ git init -q gitlinks
  $ cd gitlinks
  $ mkdir target-objects
  $ printf 'linked blob\n' | GIT_OBJECT_DIRECTORY="$PWD/target-objects" git hash-object -w --stdin > blob_oid
  $ printf 'tagged blob\n' | GIT_OBJECT_DIRECTORY="$PWD/target-objects" git hash-object -w --stdin > tagged_blob_oid
  $ printf '100644 blob %s\tinside.txt\n' "$(cat blob_oid)" | GIT_OBJECT_DIRECTORY="$PWD/target-objects" git mktree > nested_tree_oid
  $ GIT_AUTHOR_DATE='1112911993 +0000' GIT_COMMITTER_DATE='1112911993 +0000' GIT_OBJECT_DIRECTORY="$PWD/target-objects" git commit-tree "$(cat nested_tree_oid)" -m linked > commit_oid
  $ printf 'object %s\ntype blob\ntag linked\ntagger Josh <josh@example.com> 1112911993 +0000\n\nlinked tag\n' "$(cat tagged_blob_oid)" | GIT_OBJECT_DIRECTORY="$PWD/target-objects" git mktag > tag_oid
  $ printf '160000 commit %s\tblob-ref\n160000 commit %s\tblob-repeat\n160000 commit %s\tcommit-ref\n160000 commit %s\tmissing-ref\n160000 commit %s\ttag-ref\n160000 commit %s\ttree-ref\n' "$(cat blob_oid)" "$(cat blob_oid)" "$(cat commit_oid)" 1111111111111111111111111111111111111111 "$(cat tag_oid)" "$(cat nested_tree_oid)" | git mktree --missing > gitlinks_tree_oid
  $ cp -R target-objects/. .git/objects/

  $ git-tree-pretty "$(cat gitlinks_tree_oid)"
  .
  ├── blob-ref ⇒ [blob a804efc]
  ├── blob-repeat ⇒ [blob a804efc]
  ├── commit-ref ⇒ [commit 581b6ea, tree 591b27d]
  ├── missing-ref ⇒ [unavailable 1111111]
  ├── tag-ref ⇒ [tag e40a4e0]
  └── tree-ref ⇒ [tree 591b27d]

  $ git-tree-pretty --follow-gitlinks "$(cat gitlinks_tree_oid)"
  .
  ├── blob-ref ⇒ [blob a804efc]
  │   ┆  linked blob
  ├── blob-repeat ⇒ [blob a804efc, shown above]
  ├── commit-ref ⇒ [commit 581b6ea, tree 591b27d]
  │   └── inside.txt
  │       ┆  linked blob
  ├── missing-ref ⇒ [unavailable 1111111]
  ├── tag-ref ⇒ [tag e40a4e0]
  │   └─▶ [blob dc9c580]
  │       ┆  tagged blob
  └── tree-ref ⇒ [tree 591b27d]
      └── inside.txt
          ┆  linked blob
