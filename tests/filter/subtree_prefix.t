  $ git init -q 1>/dev/null

Initial commit of main branch
  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1>/dev/null

Initial commit of subtree branch
  $ git checkout --orphan subtree
  Switched to a new branch 'subtree'
  $ rm file*
  $ echo contents2 > file2
  $ git add .
  $ git commit -m "add file2 (in subtree)" 1>/dev/null
  $ export SUBTREE_TIP=$(git rev-parse HEAD)

Articially create a subtree merge
(merge commit has subtree files in subfolder but has subtree commit as a parent)
  $ git checkout master
  Switched to branch 'master'
  $ git merge subtree --allow-unrelated-histories 1>/dev/null
  $ mkdir subtree
  $ git mv file2 subtree/
  $ git add subtree
  $ git commit -a --amend -m "subtree merge" 1>/dev/null
  $ tree
  .
  |-- file1
  `-- subtree
      `-- file2
  
  1 directory, 2 files
  $ git log --graph --pretty=%s
  *   subtree merge
  |\  
  | * add file2 (in subtree)
  * add file1

Change subtree file
  $ echo more contents >> subtree/file2
  $ git commit -a -m "subtree edit from main repo" 1>/dev/null

Rewrite the subtree part of the history
  $ josh-filter -s :at_commit=$SUBTREE_TIP[:prefix=subtree] refs/heads/master --update refs/heads/filtered
  [1] :prefix=subtree
  [4] :at_commit=c036f944faafb865e0585e4fa5e005afa0aeea3f[:prefix=subtree]

  $ git log --graph --pretty=%s refs/heads/filtered
  * subtree edit from main repo
  *   subtree merge
  |\  
  | * add file2 (in subtree)
  * add file1

Compare input and result. ^^2 is the 2nd parent of the first parent, i.e., the 'in subtree' commit.
  $ git ls-tree --name-only -r refs/heads/filtered
  file1
  subtree/file2
  $ git diff refs/heads/master refs/heads/filtered
  $ git ls-tree --name-only -r refs/heads/filtered^^2
  subtree/file2
  $ git diff refs/heads/master^^2 refs/heads/filtered^^2
  diff --git a/file2 b/subtree/file2
  similarity index 100%
  rename from file2
  rename to subtree/file2

Extract the subtree history
  $ josh-filter -s :at_commit=$SUBTREE_TIP[:prefix=subtree]:/subtree refs/heads/master --update refs/heads/subtree
  [1] :prefix=subtree
  [4] :/subtree
  [4] :at_commit=c036f944faafb865e0585e4fa5e005afa0aeea3f[:prefix=subtree]
  $ git checkout subtree
  Switched to branch 'subtree'
  $ cat file2
  contents2
  more contents

Work in the subtree, and sync that back.
  $ echo even more contents >> file2
  $ git commit -am "add even more content" 1>/dev/null
  $ josh-filter -s :at_commit=$SUBTREE_TIP[:prefix=subtree]:/subtree refs/heads/master --update refs/heads/subtree --reverse
  [1] :prefix=subtree
  [4] :/subtree
  [4] :at_commit=c036f944faafb865e0585e4fa5e005afa0aeea3f[:prefix=subtree]
  $ git log --graph --pretty=%s  refs/heads/master
  * add even more content
  * subtree edit from main repo
  *   subtree merge
  |\  
  | * add file2 (in subtree)
  * add file1
  $ git ls-tree --name-only -r refs/heads/master
  file1
  subtree/file2
  $ git checkout master
  Switched to branch 'master'
  $ cat subtree/file2
  contents2
  more contents
  even more contents

And then re-extract, which should re-construct the same subtree.
  $ josh-filter -s :at_commit=$SUBTREE_TIP[:prefix=subtree]:/subtree refs/heads/master --update refs/heads/subtree2
  [1] :prefix=subtree
  [5] :/subtree
  [5] :at_commit=c036f944faafb865e0585e4fa5e005afa0aeea3f[:prefix=subtree]
  $ test $(git rev-parse subtree) = $(git rev-parse subtree2)
