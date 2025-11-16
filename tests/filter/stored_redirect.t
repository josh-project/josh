  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents4 > sub1/file4
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir st
  $ cat > st/config.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > EOF
  $ echo "foobar" > st/extra_file
  $ git add st
  $ git commit -m "add st" 1> /dev/null

  $ mkdir sub3
  $ echo contents3 > sub3/file4
  $ git add sub3
  $ git commit -m "add file4" 1> /dev/null

  $ cat > st/config.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > b = :/sub3
  > EOF
  $ git add st
  $ git commit -m "edit st" 1> /dev/null

  $ mkdir st_new
  $ echo "foobar" > st_new/extra_file_new
  $ cat > st_new/config.josh <<EOF
  > :+st/config
  > EOF
  $ git add st_new
  $ git commit -m "add st_new" 1> /dev/null

  $ josh-filter -s :+st/config master --update refs/heads/filtered
  [1] :prefix=b
  [2] :/sub3
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [3] :+st/config
  [7] sequence_number
  $ josh-filter -s :+st_new/config master --update refs/heads/filtered_new
  [1] :prefix=b
  [2] :+st_new/config
  [2] :/sub3
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [3] :+st/config
  [6] :exclude[::st_new/config.josh]
  [8] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  *   edit st
  |\  
  | * add file4
  * add st
  * add file2
  * add file1
  $ git log --graph --pretty=%s refs/heads/filtered_new
  *   edit st
  |\  
  | * add file4
  * add st
  * add file2
  * add file1

  $ git diff ${EMPTY_TREE}..refs/heads/filtered
  diff --git a/a/file1 b/a/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/a/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/a/file4 b/a/file4
  new file mode 100644
  index 0000000..288746e
  --- /dev/null
  +++ b/a/file4
  @@ -0,0 +1 @@
  +contents4
  diff --git a/b/file4 b/b/file4
  new file mode 100644
  index 0000000..1cb5d64
  --- /dev/null
  +++ b/b/file4
  @@ -0,0 +1 @@
  +contents3
  diff --git a/st/config.josh b/st/config.josh
  new file mode 100644
  index 0000000..795cb6d
  --- /dev/null
  +++ b/st/config.josh
  @@ -0,0 +1,3 @@
  +::sub2/subsub/
  +a = :/sub1
  +b = :/sub3
  diff --git a/sub2/subsub/file2 b/sub2/subsub/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub2/subsub/file2
  @@ -0,0 +1 @@
  +contents1
  $ git diff ${EMPTY_TREE}..refs/heads/filtered_new
  diff --git a/a/file1 b/a/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/a/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/a/file4 b/a/file4
  new file mode 100644
  index 0000000..288746e
  --- /dev/null
  +++ b/a/file4
  @@ -0,0 +1 @@
  +contents4
  diff --git a/b/file4 b/b/file4
  new file mode 100644
  index 0000000..1cb5d64
  --- /dev/null
  +++ b/b/file4
  @@ -0,0 +1 @@
  +contents3
  diff --git a/st/config.josh b/st/config.josh
  new file mode 100644
  index 0000000..795cb6d
  --- /dev/null
  +++ b/st/config.josh
  @@ -0,0 +1,3 @@
  +::sub2/subsub/
  +a = :/sub1
  +b = :/sub3
  diff --git a/sub2/subsub/file2 b/sub2/subsub/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub2/subsub/file2
  @@ -0,0 +1 @@
  +contents1


  $ cat > st/config.josh <<EOF
  > :+st_new/config
  > EOF
  $ git add st
  $ git commit -m "add st recursion" 1> /dev/null

  $ josh-filter -s :+st/config master --update refs/heads/filtered
  [1] :prefix=b
  [2] :/sub3
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [3] :+st_new/config
  [4] :+st/config
  [5] :exclude[::st/config.josh]
  [9] :exclude[::st_new/config.josh]
  [13] sequence_number

