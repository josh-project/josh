  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir xx
  $ echo contents1 > xx/file2
  $ git add xx
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir -p sub/xx
  $ echo contents1 > sub/xx/file3
  $ echo contents1 > sub/xx/file4
  $ git add sub
  $ git commit -m "add file3" 1> /dev/null

  $ git diff ${EMPTY_TREE}..HEAD
  diff --git a/sub/xx/file3 b/sub/xx/file3
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub/xx/file3
  @@ -0,0 +1 @@
  +contents1
  diff --git a/sub/xx/file4 b/sub/xx/file4
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub/xx/file4
  @@ -0,0 +1 @@
  +contents1
  diff --git a/sub1/file1 b/sub1/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub1/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/xx/file2 b/xx/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/xx/file2
  @@ -0,0 +1 @@
  +contents1


  $ josh-filter ":[:/sub1,:/xx]"
  $ git diff ${EMPTY_TREE}..FILTERED_HEAD
  diff --git a/file1 b/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/file2 b/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file2
  @@ -0,0 +1 @@
  +contents1

  $ josh-filter ":[:/xx,:/sub1]"
  $ git diff ${EMPTY_TREE}..FILTERED_HEAD
  diff --git a/file1 b/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/file2 b/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file2
  @@ -0,0 +1 @@
  +contents1

  $ josh-filter -s ":[:/sub/xx::file3,:/sub1,:/xx,:/sub/xx]"
  [1] :/sub1
  [1] ::file3
  [2] :/sub
  [3] :/xx
  [3] :[
      :/sub/xx::file3
      :/sub1
      :/xx
      :/sub/xx
  ]
  [3] :[
      :/xx
      :/sub/xx
  ]
  $ git diff ${EMPTY_TREE}..FILTERED_HEAD
  diff --git a/file1 b/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/file2 b/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file2
  @@ -0,0 +1 @@
  +contents1
  diff --git a/file3 b/file3
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file3
  @@ -0,0 +1 @@
  +contents1
  diff --git a/file4 b/file4
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file4
  @@ -0,0 +1 @@
  +contents1
