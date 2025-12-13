  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir sub1
  mkdir: cannot create directory 'sub1': File exists
  [1]
  $ echo contents2 > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ git log --graph --pretty=%H
  * 81b10fb4984d20142cd275b89c91c346e536876a
  * bb282e9cdc1b972fffd08fd21eead43bc0c83cb8

  $ mkdir table
  $ echo ":prefix=x" > table/81b10fb4984d20142cd275b89c91c346e536876a
  $ echo ":prefix=y" > table/bb282e9cdc1b972fffd08fd21eead43bc0c83cb8
  $ git add table
  $ git commit -m "add lookup table" 1> /dev/null


  $ echo contents3 > sub1/file3
  $ git add sub1
  $ git commit -m "add file3" 1> /dev/null

  $ git log --graph --pretty=%H
  * 26e4c43675b985689e280bc42264a9226af76943
  * 14c74c5eca73952b36d736034b388832748c49d6
  * 81b10fb4984d20142cd275b89c91c346e536876a
  * bb282e9cdc1b972fffd08fd21eead43bc0c83cb8

  $ josh-filter -s ":lookup=table" --update refs/heads/filtered
  [1] :lookup=table
  [2] :/table
  [4] :lookup2=4880528e9d57aa5efc925e120a8077bfa37d778d

  $ git log refs/heads/filtered --graph --pretty=%s
  * add file2
  * add file1
  $ git diff ${EMPTY_TREE}..refs/heads/filtered
  diff --git a/x/sub1/file1 b/x/sub1/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/x/sub1/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/x/sub1/file2 b/x/sub1/file2
  new file mode 100644
  index 0000000..6b46faa
  --- /dev/null
  +++ b/x/sub1/file2
  @@ -0,0 +1 @@
  +contents2
  $ git diff ${EMPTY_TREE}..refs/heads/filtered~1
  diff --git a/y/sub1/file1 b/y/sub1/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/y/sub1/file1
  @@ -0,0 +1 @@
  +contents1

  $ echo ":prefix=z" > table/14c74c5eca73952b36d736034b388832748c49d6
  $ echo ":prefix=z" > table/26e4c43675b985689e280bc42264a9226af76943
  $ git add table
  $ git commit -m "mod lookup table" 1> /dev/null
  $ tree table
  table
  |-- 14c74c5eca73952b36d736034b388832748c49d6
  |-- 26e4c43675b985689e280bc42264a9226af76943
  |-- 81b10fb4984d20142cd275b89c91c346e536876a
  `-- bb282e9cdc1b972fffd08fd21eead43bc0c83cb8
  
  1 directory, 4 files

  $ josh-filter -s ":lookup=table" --update refs/heads/filtered
  Warning: reference refs/heads/filtered wasn't updated
  [2] :lookup=table
  [3] :/table
  [4] :lookup2=4880528e9d57aa5efc925e120a8077bfa37d778d
  [5] :lookup2=ed934c124e28c83270d9cfbb011f3ceb46c0f69e
  $ git log refs/heads/filtered --graph --pretty=%s
  * add file2
  * add file1

  $ git diff ${EMPTY_TREE}..refs/heads/filtered
  diff --git a/x/sub1/file1 b/x/sub1/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/x/sub1/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/x/sub1/file2 b/x/sub1/file2
  new file mode 100644
  index 0000000..6b46faa
  --- /dev/null
  +++ b/x/sub1/file2
  @@ -0,0 +1 @@
  +contents2
  $ git diff ${EMPTY_TREE}..refs/heads/filtered~1
  diff --git a/y/sub1/file1 b/y/sub1/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/y/sub1/file1
  @@ -0,0 +1 @@
  +contents1
