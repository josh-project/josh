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

  $ josh-filter -s ":rev(ffffffffffffffffffffffffffffffffffffffff:prefix=x/y)" --update refs/heads/filtered
  [5] :prefix=x
  [5] :prefix=y
  ERROR: `:rev(...)` with nonexistent OID: ffffffffffffffffffffffffffffffffffffffff
  [1]

  $ josh-filter -s ":rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)" --update refs/heads/filtered
  [5] :prefix=x
  [5] :prefix=y
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  *   54651c29aa86e8512a7b9d39e3b8ea26da644247:5f47d9fdffdc726bb8ebcfea67531d2574243c5d
  |\  
  | * ee931ac07e4a953d1d2e0f65968946f5c09b0f4c:5d0da4f47308da86193b53b3374f5630c5a0fa3e
  | * cc0382917c6488d69dca4d6a147d55251b06ac08:8408d8fc882cba8e945b16bc69e3b475d65ecbeb
  * | daf46738b8fddd211a1609bf3b9de339fe7589eb:5d8a699f74b48c9c595f4615dd3755244e11d176
  |/  
  * 9f0db868b59a422c114df33bc6a8b2950f80490b:a087bfbdb1a5bad499b40ccd1363d30db1313f54


  $ josh-filter -s ":rev(e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)" --update refs/heads/filtered
  [5] :prefix=x
  [5] :prefix=y
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [5] :rev(e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  *   5fe60a2d55b652822b3d3f25410714e9053ba72b:5f47d9fdffdc726bb8ebcfea67531d2574243c5d
  |\  
  | * 0822879dab0a93f29848500e72642d6c8c0db162:5d0da4f47308da86193b53b3374f5630c5a0fa3e
  | * 5c145ed574623e7687f4c7a5d1d40b48687bf17c:de6937d89a7433c80125962616db5dca6c206d9d
  * | 08158c6ba260a65db99c1e9e6f519e1963dff07b:6d18321f410e431cd446258dd5e01999306d9d44
  |/  
  * 9f0db868b59a422c114df33bc6a8b2950f80490b:a087bfbdb1a5bad499b40ccd1363d30db1313f54
  $ cat > filter.josh <<EOF
  > :rev(
  >   e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y
  >   975d4c4975912729482cc864d321c5196a969271:prefix=x/y
  > )
  > EOF
  $ josh-filter -s --file filter.josh --update refs/heads/filtered
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :prefix=x
  [5] :prefix=y
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  *   63fea1234f375bd09019b676da8291f28d2ddb43:5f47d9fdffdc726bb8ebcfea67531d2574243c5d
  |\  
  | * ee931ac07e4a953d1d2e0f65968946f5c09b0f4c:5d0da4f47308da86193b53b3374f5630c5a0fa3e
  | * cc0382917c6488d69dca4d6a147d55251b06ac08:8408d8fc882cba8e945b16bc69e3b475d65ecbeb
  * | 08158c6ba260a65db99c1e9e6f519e1963dff07b:6d18321f410e431cd446258dd5e01999306d9d44
  |/  
  * 9f0db868b59a422c114df33bc6a8b2950f80490b:a087bfbdb1a5bad499b40ccd1363d30db1313f54
  $ cat > filter.josh <<EOF
  > :rev(
  >     e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y
  >     975d4c4975912729482cc864d321c5196a969271:prefix=x/z
  > )
  > EOF
  $ josh-filter -s --file filter.josh --update refs/heads/filtered
  [1] :prefix=z
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/z)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :prefix=y
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [6] :prefix=x
  $ cat > filter.josh <<EOF
  > :rev(
  >   e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y
  >   975d4c4975912729482cc864d321c5196a969271:prefix=x/z
  > )
  > EOF
  $ josh-filter -s --file filter.josh --update refs/heads/filtered
  Warning: reference refs/heads/filtered wasn't updated
  [1] :prefix=z
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/z)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :prefix=y
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [6] :prefix=x
  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  *   1c4fe25dc386c77adaae12d6b1cd3abfa296fc3c:5f47d9fdffdc726bb8ebcfea67531d2574243c5d
  |\  
  | * 17a13131b354b75d39aa29896f0500ac1b5e6764:5d0da4f47308da86193b53b3374f5630c5a0fa3e
  | * 8516b8e4396bc91c72cec0038325d82604e8d685:b9d380f578c1cb2bb5039977f64ccf1a804a91de
  | * 9f0db868b59a422c114df33bc6a8b2950f80490b:a087bfbdb1a5bad499b40ccd1363d30db1313f54
  * 74a368bd558785377d64ecdb3a47f2d1b4f25113:6d18321f410e431cd446258dd5e01999306d9d44
  * 26cbb56df84c5e9fdce7afc7855025862e835ee2:105b58b790c53d350e23a51ad763a88e6b977ae7

  $ josh-filter -s :linear --update refs/heads/filtered
  [1] :prefix=z
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/z)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [3] :linear
  [5] :prefix=y
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [6] :prefix=x
  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  * f8e8bc9daf54340c9fce647be467d2577b623bbe:5f47d9fdffdc726bb8ebcfea67531d2574243c5d
  * e707f76bb6a1390f28b2162da5b5eb6933009070:5d8a699f74b48c9c595f4615dd3755244e11d176
  * 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:3d77ff51363c9825cc2a221fc0ba5a883a1a2c72

  $ git diff --stat ${EMPTY_TREE}..f8e8bc9daf54340c9fce647be467d2577b623bbe
   file1 | 1 +
   file2 | 1 +
   file3 | 1 +
   3 files changed, 3 insertions(+)
  $ git diff --stat ${EMPTY_TREE}..e707f76bb6a1390f28b2162da5b5eb6933009070
   file1 | 1 +
   file2 | 1 +
   2 files changed, 2 insertions(+)
  $ git diff --stat ${EMPTY_TREE}..0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb
   file1 | 1 +
   1 file changed, 1 insertion(+)

  $ cat > filter.josh <<EOF
  > :linear:rev(
  >   0000000000000000000000000000000000000000:prefix=x
  >   e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=y
  >   0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:prefix=z
  > )
  > EOF
  $ josh-filter -s --file filter.josh --update refs/heads/filtered
  [1] :prefix=z
  [1] :rev(0000000000000000000000000000000000000000:prefix=z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,975d4c4975912729482cc864d321c5196a969271:prefix=x/z)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [2] :rev(0000000000000000000000000000000000000000:prefix=y,0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:prefix=z)
  [3] :linear
  [3] :rev(0000000000000000000000000000000000000000:prefix=x,0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb:prefix=z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=y)
  [5] :prefix=y
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/y,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(975d4c4975912729482cc864d321c5196a969271:prefix=x/z,e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [5] :rev(e707f76bb6a1390f28b2162da5b5eb6933009070:prefix=x/y)
  [6] :prefix=x

  $ git log --graph --decorate --pretty=%H:%T refs/heads/filtered
  * 2944f04c33ea037f7696282bf20b2e570524552e:047b1b6f39e8d95b62ef7f136189005d0e3c80b3
  * 3c2304baa035aa9c8e7e0f1fff5d7410be55f069:6300cae79def8ee31701b104857ff4338b6079aa
  * 67480de4b94241494bfb0d7f606d421d8ed4f7e6:2fd6d8f78756533e937e3f168eb58e0fd8b1512c

  $ git diff --stat ${EMPTY_TREE}..refs/heads/filtered
   x/file1 | 1 +
   x/file2 | 1 +
   x/file3 | 1 +
   3 files changed, 3 insertions(+)
  $ git diff --stat ${EMPTY_TREE}..refs/heads/filtered~1
   y/file1 | 1 +
   y/file2 | 1 +
   2 files changed, 2 insertions(+)
  $ git diff --stat ${EMPTY_TREE}..refs/heads/filtered~2
   z/file1 | 1 +
   1 file changed, 1 insertion(+)
