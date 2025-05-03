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

Artificially create a subtree merge
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
  
  2 directories, 2 files
  $ git log --graph --pretty=%s
  *   subtree merge
  |\  
  | * add file2 (in subtree)
  * add file1

Change subtree file
  $ echo more contents >> subtree/file2
  $ git commit -a -m "subtree edit from main repo" 1>/dev/null

Rewrite the subtree part of the history
  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree)" refs/heads/master --update refs/heads/filtered
  [1] :prefix=subtree
  [4] :rev(c036f944faafb865e0585e4fa5e005afa0aeea3f:prefix=subtree)

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
  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree):/subtree" refs/heads/master --update refs/heads/subtree
  [1] :prefix=subtree
  [4] :/subtree
  [4] :rev(c036f944faafb865e0585e4fa5e005afa0aeea3f:prefix=subtree)
  $ git checkout subtree
  Switched to branch 'subtree'
  $ cat file2
  contents2
  more contents

Work in the subtree, and sync that back.
  $ echo even more contents >> file2
  $ git commit -am "add even more content" 1>/dev/null
  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree):/subtree" refs/heads/master --update refs/heads/subtree --reverse
  [1] :prefix=subtree
  [4] :/subtree
  [4] :rev(c036f944faafb865e0585e4fa5e005afa0aeea3f:prefix=subtree)
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
  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree):/subtree" refs/heads/master --update refs/heads/subtree2
  [1] :prefix=subtree
  [5] :/subtree
  [5] :rev(c036f944faafb865e0585e4fa5e005afa0aeea3f:prefix=subtree)
  $ test $(git rev-parse subtree) = $(git rev-parse subtree2)

Simulate a feature branch on the main repo that crosses subtree changes
  $ git checkout master 2>/dev/null
  $ git checkout -b feature1 2>/dev/null
  $ git reset --hard $SUBTREE_TIP >/dev/null
  $ echo work > feature1
  $ git add feature1 >/dev/null
  $ git commit -m feature1 >/dev/null
  $ git checkout master 2>/dev/null
  $ git merge feature1 --no-ff >/dev/null

On the subtree, simulate some independent work, and then a sync, then some more work.
  $ git checkout subtree 2>/dev/null
  $ echo work > subfeature1
  $ git add subfeature1 >/dev/null
  $ git commit -m subfeature1 >/dev/null
  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree):/subtree" refs/heads/master --update refs/heads/subtree-sync >/dev/null
  $ git merge subtree-sync --no-ff >/dev/null
  $ echo work > subfeature2
  $ git add subfeature2 >/dev/null
  $ git commit -m subfeature2 >/dev/null

And another main tree feature off of SUBTREE_TIP
  $ git checkout -b feature2 2>/dev/null
  $ git reset --hard $SUBTREE_TIP >/dev/null
  $ echo work > feature2
  $ git add feature2 >/dev/null
  $ git commit -m feature2 >/dev/null
  $ git checkout master 2>/dev/null
  $ git merge feature2 --no-ff >/dev/null

And finally, sync first from main to sub and then back.
  $ git checkout subtree 2>/dev/null
  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree):/subtree" refs/heads/master --update refs/heads/subtree-sync >/dev/null
  $ git merge subtree-sync --no-ff >/dev/null

  $ git log --graph --pretty=%s refs/heads/master
  *   Merge branch 'feature2'
  |\  
  | * feature2
  * |   Merge branch 'feature1'
  |\ \  
  | * | feature1
  | |/  
  * | add even more content
  * | subtree edit from main repo
  * | subtree merge
  |\| 
  | * add file2 (in subtree)
  * add file1
  $ git log --graph --pretty=%s refs/heads/subtree
  *   Merge branch 'subtree-sync' into subtree
  |\  
  | *   Merge branch 'feature2'
  | |\  
  | | * feature2
  * | | subfeature2
  * | | Merge branch 'subtree-sync' into subtree
  |\| | 
  | * |   Merge branch 'feature1'
  | |\ \  
  | | * | feature1
  | | |/  
  * | / subfeature1
  |/ /  
  * | add even more content
  * | subtree edit from main repo
  |/  
  * add file2 (in subtree)
  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree):/subtree" refs/heads/master --update refs/heads/subtree --reverse
  [1] :prefix=subtree
  [9] :/subtree
  [9] :rev(c036f944faafb865e0585e4fa5e005afa0aeea3f:prefix=subtree)

  $ git log --graph --pretty=%H:%s refs/heads/master
  *   6ac0ba56575859cfaacd5818084333e532ffc442:Merge branch 'subtree-sync' into subtree
  |\  
  | *   38a6d753c8b17b4c6721050befbccff012dfde85:Merge branch 'feature2'
  | |\  
  | | * 221f5ceab31209c3d3b16d5b2485ea54c465eca6:feature2
  * | | 75e90f7f1b54cc343f2f75dcdee33650654a52a6:subfeature2
  * | | 3fa497039e5b384cb44b704e6e96f52e0ae599c9:Merge branch 'subtree-sync' into subtree
  |\| | 
  | * |   2739fb8f0b3f6d5a264fb89ea20674fe34790321:Merge branch 'feature1'
  | |\ \  
  | | * | dbfaf5dd32fc39ce3c0ebe61864406bb7e2ad113:feature1
  | | |/  
  * | / 59b5c1623da3f89229c6dd36f8baf2e5868d0288:subfeature1
  |/ /  
  * | 103bfec17c47adbe70a95fca90caefb989b6cda6:add even more content
  * | 41130c5d66736545562212f820cdbfbb3d3779c4:subtree edit from main repo
  * | 0642c36d6b53f7e829531aed848e3ceff0762c64:subtree merge
  |\| 
  | * c036f944faafb865e0585e4fa5e005afa0aeea3f:add file2 (in subtree)
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:add file1

  $ git ls-tree --name-only -r 3fa497039e5b384cb44b704e6e96f52e0ae599c9
  feature1
  file1
  subtree/file2
  subtree/subfeature1

  $ git checkout subtree
  Already on 'subtree'

Create an extra file and amend the merge commit to include it, then check it is also
taken back into the main history.
  $ echo "random stuff" > a_file
  $ git add a_file
  $ git commit --amend --no-edit
  [subtree bab52d5] Merge branch 'subtree-sync' into subtree
   Date: Thu Apr 7 22:13:13 2005 +0000
  $ git checkout master
  Switched to branch 'master'

  $ josh-filter -s ":rev($SUBTREE_TIP:prefix=subtree):/subtree" refs/heads/master --update refs/heads/subtree --reverse
  [1] :prefix=subtree
  [13] :/subtree
  [13] :rev(c036f944faafb865e0585e4fa5e005afa0aeea3f:prefix=subtree)
  $ git ls-tree --name-only -r refs/heads/master
  feature1
  feature2
  file1
  subtree/a_file
  subtree/file2
  subtree/subfeature1
  subtree/subfeature2

  $ git log --graph --pretty=%H:%s refs/heads/master
  *   f814033dd0148da19a3199cd3cb2d21464ce85a3:Merge branch 'subtree-sync' into subtree
  |\  
  | *   38a6d753c8b17b4c6721050befbccff012dfde85:Merge branch 'feature2'
  | |\  
  | | * 221f5ceab31209c3d3b16d5b2485ea54c465eca6:feature2
  * | | 75e90f7f1b54cc343f2f75dcdee33650654a52a6:subfeature2
  * | | 3fa497039e5b384cb44b704e6e96f52e0ae599c9:Merge branch 'subtree-sync' into subtree
  |\| | 
  | * |   2739fb8f0b3f6d5a264fb89ea20674fe34790321:Merge branch 'feature1'
  | |\ \  
  | | * | dbfaf5dd32fc39ce3c0ebe61864406bb7e2ad113:feature1
  | | |/  
  * | / 59b5c1623da3f89229c6dd36f8baf2e5868d0288:subfeature1
  |/ /  
  * | 103bfec17c47adbe70a95fca90caefb989b6cda6:add even more content
  * | 41130c5d66736545562212f820cdbfbb3d3779c4:subtree edit from main repo
  * | 0642c36d6b53f7e829531aed848e3ceff0762c64:subtree merge
  |\| 
  | * c036f944faafb865e0585e4fa5e005afa0aeea3f:add file2 (in subtree)
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:add file1
