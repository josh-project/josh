  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ git log --graph --pretty=%s
  * add file1

  $ git checkout -b branch2
  Switched to a new branch 'branch2'

  $ echo contents2 > file1
  $ git add .
  $ git commit -m "mod file1" 1> /dev/null

  $ echo contents3 > file3
  $ git add .
  $ git commit -m "mod file3" 1> /dev/null

  $ git checkout master
  Switched to branch 'master'

  $ echo contents3 > file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null

  $ git merge -q branch2 --no-ff

  $ josh-filter -s --squash "refs/tags/*" --author "New Author" --email "new@e.mail" --update refs/heads/filtered
  Warning: reference refs/heads/filtered wasn't updated
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  fatal: ambiguous argument 'refs/heads/filtered': unknown revision or path not in the working tree.
  Use '--' to separate paths from revisions, like this:
  'git <command> [<revision>...] -- [<file>...]'
  [128]
  $ git tag tag_a 1d69b7d
  $ josh-filter -s --squash "refs/tags/*" :author=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  [1] :author="New Author";"new@e.mail"
  [2] :SQUASH=10d465cdf297e8062eed54204414414faa63671e

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  * d8aa5a9937f4f0bd645dbc0b591bae5cd6b6d91b (tag: filtered/tag_a, filtered) refs/tags/tag_a
  $ git tag tag_b 0b4cf6c


  $ git log --graph --decorate --pretty=oneline
  *   1d69b7d2651f744be3416f2ad526aeccefb99310 (HEAD -> master, tag: tag_a) Merge branch 'branch2'
  |\  
  | * 86871b8775ad3baca86484337d1072aa1d386f7e (branch2) mod file3
  | * 975d4c4975912729482cc864d321c5196a969271 mod file1
  * | e707f76bb6a1390f28b2162da5b5eb6933009070 add file2
  |/  
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb (tag: tag_b) add file1

  $ josh-filter -s --squash "refs/tags/*" :author=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  [2] :SQUASH=10d465cdf297e8062eed54204414414faa63671e
  [3] :SQUASH=dd8bdf1d78a6cb9ffc9e2a0644a8bf41de56ad36
  [4] :author="New Author";"new@e.mail"

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  * 5b1a753860ca124024f6dfb4fd018fe7df8beae4 (tag: filtered/tag_a, filtered) refs/tags/tag_a
  |\
  * 96a731a4d64a8928e6af7abb2d425df3812b4197 (tag: filtered/tag_b) refs/tags/tag_b

  $ git log --graph --pretty=%an:%ae refs/heads/master
  *   Josh:josh@example.com
  |\  
  | * Josh:josh@example.com
  | * Josh:josh@example.com
  * | Josh:josh@example.com
  |/  
  * Josh:josh@example.com
  $ git log --graph --pretty=%an:%ae refs/heads/filtered
  * New Author:new@e.mail
  |\
  * New Author:new@e.mail

  $ git tag tag_c 975d4c4

  $ josh-filter -s --squash "refs/tags/*" :author=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  [2] :SQUASH=10d465cdf297e8062eed54204414414faa63671e
  [3] :SQUASH=dd8bdf1d78a6cb9ffc9e2a0644a8bf41de56ad36
  [6] :SQUASH=b2a9a51df03600d3b5858fa7fca044741f88e521
  [9] :author="New Author";"new@e.mail"

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  *   9fe45cb2bead844630852ab338ecd8e073f8ba50 (tag: filtered/tag_a, filtered) refs/tags/tag_a
  |\  
  | * d6b88d4c1cc566b7f4d9b51353ec6f3204a93b81 (tag: filtered/tag_c) refs/tags/tag_c
  |/  
  * 96a731a4d64a8928e6af7abb2d425df3812b4197 (tag: filtered/tag_b) refs/tags/tag_b

