  $ export RUST_BACKTRACE=1
  $ git init -q 1> /dev/null

  $ git commit -m "Empty initial" --allow-empty 1> /dev/null

  $ git log --graph --pretty=%s
  * Empty initial

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

  $ git log --graph --decorate --pretty=oneline
  *   882f2656a5075936eb37bfefde740e0b453e4479 (HEAD -> master) Merge branch 'branch2'
  |\  
  | * 87bb87b63d1745136cb2ea167ef3ffc82c7ef3f0 (branch2) mod file3
  | * 2db14eafe99deeeab5db07bf33e332d523a298ab mod file1
  * | 54d8f704681c3b44a468cef655fa3b5bc5229a4c add file2
  |/  
  * 8c26fa0172bda17bafcbcf9684e639c6b0bae9c4 Empty initial

  $ josh-filter -s --squash-pattern "refs/tags/*" --update refs/heads/filtered
  Warning: reference refs/heads/filtered wasn't updated
  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  fatal: ambiguous argument 'refs/heads/filtered': unknown revision or path not in the working tree.
  Use '--' to separate paths from revisions, like this:
  'git <command> [<revision>...] -- [<file>...]'
  [128]

  $ git tag -a tag_a -m "created a tag" 882f2656a5075936eb37bfefde740e0b453e4479
  $ josh-filter -s --squash-pattern "refs/tags/*" :author=\"New\ Author\"\;\"new@e.mail\" --update refs/heads/filtered
  [1] :"refs/tags/tag_a"
  [1] :author="New Author";"new@e.mail"
  [1] :squash(
      882f2656a5075936eb37bfefde740e0b453e4479:"refs/tags/tag_a"
  )

  $ git log --graph --decorate --pretty=oneline refs/heads/filtered
  * d8aa5a9937f4f0bd645dbc0b591bae5cd6b6d91b (tag: filtered/tag_a, filtered) refs/tags/tag_a
