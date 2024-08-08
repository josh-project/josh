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

  $ josh-filter -s --squash-pattern "refs/tags/*" --update refs/heads/filtered
  Warning: reference refs/heads/filtered wasn't updated

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  fatal: ambiguous argument 'refs/heads/filtered': unknown revision or path not in the working tree.
  Use '--' to separate paths from revisions, like this:
  'git <command> [<revision>...] -- [<file>...]'
  [128]

This one tag is an annotated tag, to make sure those are handled as well
  $ git tag -a tag_a -m "created a tag" 1d69b7d
  $ josh-filter -s --squash-pattern "refs/tags/*" :author=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered
  [1] :"refs/tags/tag_a"
  [1] :author="New Author";"new@e.mail"
  [1] :squash(
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
  )

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  * 977cc3ee14c0d6163ba63bd96f4aeedd43916ba7 (tag: filtered/tag_a, filtered) refs/tags/tag_a
  $ git tag tag_b 0b4cf6c


  $ git log --graph --decorate --pretty=oneline
  *   1d69b7d2651f744be3416f2ad526aeccefb99310 (HEAD -> master, tag: tag_a) Merge branch 'branch2'
  |\  
  | * 86871b8775ad3baca86484337d1072aa1d386f7e (branch2) mod file3
  | * 975d4c4975912729482cc864d321c5196a969271 mod file1
  * | e707f76bb6a1390f28b2162da5b5eb6933009070 add file2
  |/  
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb (tag: tag_b) add file1

  $ josh-filter -s --squash-pattern "refs/tags/*" :author=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered
  [1] :"refs/tags/filtered/tag_a"
  [1] :"refs/tags/tag_a"
  [1] :"refs/tags/tag_b"
  [1] :squash(
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
  )
  [3] :squash(
      0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:"refs/tags/tag_b"
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
      977cc3ee14c0d6163ba63bd96f4aeedd43916ba7:"refs/tags/filtered/tag_a"
  )
  [4] :author="New Author";"new@e.mail"

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  * be41caf35896090033cfd103e06aae721a3ce541 (tag: filtered/tag_a, filtered) refs/tags/tag_a
  |\
  * 64f712c4615dbf5e9e0a1c4cdf65b2da2138f4be (tag: filtered/tag_b) refs/tags/tag_b

  $ git log --graph --pretty=%an:%ae-%cn:%ce refs/heads/master
  *   Josh:josh@example.com-Josh:josh@example.com
  |\  
  | * Josh:josh@example.com-Josh:josh@example.com
  | * Josh:josh@example.com-Josh:josh@example.com
  * | Josh:josh@example.com-Josh:josh@example.com
  |/  
  * Josh:josh@example.com-Josh:josh@example.com
  $ git log --graph --pretty=%an:%ae-%cn:%ce refs/heads/filtered
  * New Author:new@e.mail-Josh:josh@example.com
  |\
  * New Author:new@e.mail-Josh:josh@example.com

  $ josh-filter -s --squash-pattern "refs/tags/*" :committer=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered
  [1] :"refs/tags/filtered/filtered/tag_a"
  [1] :"refs/tags/filtered/tag_a"
  [1] :"refs/tags/filtered/tag_b"
  [1] :"refs/tags/tag_a"
  [1] :"refs/tags/tag_b"
  [1] :squash(
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
  )
  [3] :squash(
      0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:"refs/tags/tag_b"
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
      977cc3ee14c0d6163ba63bd96f4aeedd43916ba7:"refs/tags/filtered/tag_a"
  )
  [4] :author="New Author";"new@e.mail"
  [5] :committer="New Author";"new@e.mail"
  [5] :squash(
      0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:"refs/tags/tag_b"
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
      64f712c4615dbf5e9e0a1c4cdf65b2da2138f4be:"refs/tags/filtered/tag_b"
      a68763bdf2f45a44304067954855749e366a5533:"refs/tags/filtered/filtered/tag_a"
      be41caf35896090033cfd103e06aae721a3ce541:"refs/tags/filtered/tag_a"
  )
  $ git log --graph --pretty=%an:%ae-%cn:%ce refs/heads/filtered
  * Josh:josh@example.com-New Author:new@e.mail
  |\
  * Josh:josh@example.com-New Author:new@e.mail

  $ git tag tag_c 975d4c4

  $ git show-ref | grep refs/heads > squashlist
  $ cat squashlist
  86871b8775ad3baca86484337d1072aa1d386f7e refs/heads/branch2
  97c6007771c497c9530d61aa89af663daebb1625 refs/heads/filtered
  1d69b7d2651f744be3416f2ad526aeccefb99310 refs/heads/master
  $ josh-filter -s --squash-file squashlist :author=\"John\ Doe\"\;\"new@e.mail\" --update refs/heads/filtered -p > filter.josh
  $ cat filter.josh
  :squash(
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/heads/master"
      86871b8775ad3baca86484337d1072aa1d386f7e:"refs/heads/branch2"
      97c6007771c497c9530d61aa89af663daebb1625:"refs/heads/filtered"
  ):author="John Doe";"new@e.mail"

  $ josh-filter -s --squash-pattern "refs/tags/*" :author=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered -p > filter.josh
  $ cat filter.josh
  :squash(
      0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:"refs/tags/tag_b"
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
      1dd879133bc80f7d180bd98268412f8ee61226f2:"refs/tags/filtered/tag_b"
      975d4c4975912729482cc864d321c5196a969271:"refs/tags/tag_c"
      97c6007771c497c9530d61aa89af663daebb1625:"refs/tags/filtered/tag_a"
      a91f2e4061d13b9adcb6d8ca63e17c8bbc5bed55:"refs/tags/filtered/filtered/tag_a"
      b7e3b7815c4d7c8738545526b20308b1240137c7:"refs/tags/filtered/filtered/filtered/tag_a"
      c4215db39f3cd96f07fe4c1f701dad39d5f5dec3:"refs/tags/filtered/filtered/tag_b"
  ):author="New Author";"new@e.mail"
  $ josh-filter -s --file filter.josh --update refs/heads/filtered
  [1] :"refs/tags/filtered/filtered/tag_a"
  [1] :"refs/tags/filtered/tag_a"
  [1] :"refs/tags/filtered/tag_b"
  [1] :"refs/tags/tag_a"
  [1] :"refs/tags/tag_b"
  [1] :"refs/tags/tag_c"
  [1] :squash(
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
  )
  [3] :squash(
      0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:"refs/tags/tag_b"
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
      1dd879133bc80f7d180bd98268412f8ee61226f2:"refs/tags/filtered/tag_b"
      975d4c4975912729482cc864d321c5196a969271:"refs/tags/tag_c"
      97c6007771c497c9530d61aa89af663daebb1625:"refs/tags/filtered/tag_a"
      a91f2e4061d13b9adcb6d8ca63e17c8bbc5bed55:"refs/tags/filtered/filtered/tag_a"
      b7e3b7815c4d7c8738545526b20308b1240137c7:"refs/tags/filtered/filtered/filtered/tag_a"
      c4215db39f3cd96f07fe4c1f701dad39d5f5dec3:"refs/tags/filtered/filtered/tag_b"
  )
  [3] :squash(
      0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:"refs/tags/tag_b"
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
      977cc3ee14c0d6163ba63bd96f4aeedd43916ba7:"refs/tags/filtered/tag_a"
  )
  [5] :committer="New Author";"new@e.mail"
  [5] :squash(
      0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:"refs/tags/tag_b"
      1d69b7d2651f744be3416f2ad526aeccefb99310:"refs/tags/tag_a"
      64f712c4615dbf5e9e0a1c4cdf65b2da2138f4be:"refs/tags/filtered/tag_b"
      a68763bdf2f45a44304067954855749e366a5533:"refs/tags/filtered/filtered/tag_a"
      be41caf35896090033cfd103e06aae721a3ce541:"refs/tags/filtered/tag_a"
  )
  [6] :author="New Author";"new@e.mail"

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  *   2826b9a173c7a7d5c83d9ae2614de89c77205d83 (filtered) refs/tags/tag_a
  |\  
  | * 63f8653625759f860ee31cce2d4e207974da1c37 refs/tags/tag_c
  |/  
  * 64f712c4615dbf5e9e0a1c4cdf65b2da2138f4be refs/tags/tag_b

