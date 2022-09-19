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

  $ josh-filter -s --squash "refs/tags/*" --update refs/heads/filtered
  Warning: reference refs/heads/filtered wasn't updated
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  fatal: ambiguous argument 'refs/heads/filtered': unknown revision or path not in the working tree.
  Use '--' to separate paths from revisions, like this:
  'git <command> [<revision>...] -- [<file>...]'
  [128]
  $ git tag tag_a 1d69b7d
  $ josh-filter -s --squash "refs/tags/*" --update refs/heads/filtered
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  [2] :SQUASH=10d465cdf297e8062eed54204414414faa63671e

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  * 97a9ff7bd4dad25b9dacdfdaeb861e74e7b4aef8 (tag: filtered/tag_a, filtered) refs/tags/tag_a
  $ git tag tag_b 0b4cf6c

  $ git log --graph --decorate --pretty=oneline
  *   1d69b7d2651f744be3416f2ad526aeccefb99310 (HEAD -> master, tag: tag_a) Merge branch 'branch2'
  |\  
  | * 86871b8775ad3baca86484337d1072aa1d386f7e (branch2) mod file3
  | * 975d4c4975912729482cc864d321c5196a969271 mod file1
  * | e707f76bb6a1390f28b2162da5b5eb6933009070 add file2
  |/  
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb (tag: tag_b) add file1

  $ josh-filter -s --squash "refs/tags/*" --update refs/heads/filtered
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  [2] :SQUASH=10d465cdf297e8062eed54204414414faa63671e
  [3] :SQUASH=1683a7fc84387b56a1a5de8e9fcf720166951949

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  * fa3bd3f0d90d5894c6ac402ef5f764b75335ec01 (tag: filtered/tag_a, filtered) refs/tags/tag_a
  |\
  * 077b2cad7b3fbc393b6320b90c9c0be1255ac309 (tag: filtered/tag_b) refs/tags/tag_b

  $ git tag tag_c 975d4c4

  $ josh-filter -s --squash "refs/tags/*" --update refs/heads/filtered
  [1] :SQUASH=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  [2] :SQUASH=10d465cdf297e8062eed54204414414faa63671e
  [3] :SQUASH=1683a7fc84387b56a1a5de8e9fcf720166951949
  [6] :SQUASH=06a82cb9d2d3abb0ac59f8c782fd7edecc8e8d28

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  *   dc1dc0211db7a1aea1234af950b4946afa5a6f14 (tag: filtered/tag_a, filtered) refs/tags/tag_a
  |\  
  | * 500760f4e4f3d4ba6e73af7ce0a98d91a25a503a (tag: filtered/tag_c) refs/tags/tag_c
  |/  
  * 077b2cad7b3fbc393b6320b90c9c0be1255ac309 (tag: filtered/tag_b) refs/tags/tag_b

