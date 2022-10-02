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

  $ git log --graph --pretty=%H
  *   1d69b7d2651f744be3416f2ad526aeccefb99310
  |\  
  | * 86871b8775ad3baca86484337d1072aa1d386f7e
  | * 975d4c4975912729482cc864d321c5196a969271
  * | e707f76bb6a1390f28b2162da5b5eb6933009070
  |/  
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb

  $ josh-filter -s :at_commit=975d4c4975912729482cc864d321c5196a969271[:prefix=x/y] --update refs/heads/filtered
  [2] :prefix=x
  [2] :prefix=y
  [5] :at_commit=975d4c4975912729482cc864d321c5196a969271[:prefix=x/y]

  $ git log --graph --decorate --pretty=%H refs/heads/filtered
  *   8b4097f3318cdf47e46266fc7fef5331bf189b6c
  |\  
  | * ee931ac07e4a953d1d2e0f65968946f5c09b0f4c
  | * cc0382917c6488d69dca4d6a147d55251b06ac08
  | * 9f0db868b59a422c114df33bc6a8b2950f80490b
  * e707f76bb6a1390f28b2162da5b5eb6933009070
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb
