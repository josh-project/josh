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
  $ josh-filter -s :prefix=x/y --update refs/heads/filtered
  [5] :prefix=x
  [5] :prefix=y
  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  *   37f8b29c9e892ea0eb7abac2759ddc6fb0337203:dcbbddf47649f8e73f59fae92896c0d2cd02b6ec
  |\  
  | * 714ed7037ce6a45f7342e2cc1a9bb644bb616c45:67e0ba73689ea02220cb270c5b5db564e520fce3
  | * cc0382917c6488d69dca4d6a147d55251b06ac08:8408d8fc882cba8e945b16bc69e3b475d65ecbeb
  * | 08158c6ba260a65db99c1e9e6f519e1963dff07b:6d18321f410e431cd446258dd5e01999306d9d44
  |/  
  * 9f0db868b59a422c114df33bc6a8b2950f80490b:a087bfbdb1a5bad499b40ccd1363d30db1313f54

  $ josh-filter -s :at_commit=975d4c4975912729482cc864d321c5196a969271[:prefix=x/y] --update refs/heads/filtered
  [5] :at_commit=975d4c4975912729482cc864d321c5196a969271[:prefix=x/y]
  [5] :prefix=x
  [5] :prefix=y

  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  *   8b4097f3318cdf47e46266fc7fef5331bf189b6c:5f47d9fdffdc726bb8ebcfea67531d2574243c5d
  |\  
  | * ee931ac07e4a953d1d2e0f65968946f5c09b0f4c:5d0da4f47308da86193b53b3374f5630c5a0fa3e
  | * cc0382917c6488d69dca4d6a147d55251b06ac08:8408d8fc882cba8e945b16bc69e3b475d65ecbeb
  | * 9f0db868b59a422c114df33bc6a8b2950f80490b:a087bfbdb1a5bad499b40ccd1363d30db1313f54
  * e707f76bb6a1390f28b2162da5b5eb6933009070:5d8a699f74b48c9c595f4615dd3755244e11d176
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:3d77ff51363c9825cc2a221fc0ba5a883a1a2c72

  $ josh-filter -s :at_tree=de6937d89a7433c80125962616db5dca6c206d9d[:prefix=x/y] --update refs/heads/filtered
  [5] :at_commit=975d4c4975912729482cc864d321c5196a969271[:prefix=x/y]
  [5] :at_tree=de6937d89a7433c80125962616db5dca6c206d9d[:prefix=x/y]
  [5] :prefix=x
  [5] :prefix=y

  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  *   0dbcbaf2d2ac83f020fb8e782beb2786d03da8f2:5f47d9fdffdc726bb8ebcfea67531d2574243c5d
  |\  
  | * 719d248112b9dda4c830b95ab005a3a129ce3f53:5d0da4f47308da86193b53b3374f5630c5a0fa3e
  | * 7724edfd3a45da676f947fd9468d0d3c0ecbd243:8408d8fc882cba8e945b16bc69e3b475d65ecbeb
  * | e707f76bb6a1390f28b2162da5b5eb6933009070:5d8a699f74b48c9c595f4615dd3755244e11d176
  |/  
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:3d77ff51363c9825cc2a221fc0ba5a883a1a2c72
