  $ echo contents3 >> c/d/e/file_cd3
  $ git add c
  $ git commit -m "edit file_cd3" 1> /dev/null

  * edit file_cd3
  $ josh-filter -s :PATHS master --update refs/josh/filtered
  [3] :PATHS
  * add file_cd2
  |   |-- file_a2
  |   `-- file_b1
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..b5fbe37
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1 @@
  +a/workspace.josh
  \ No newline at end of file
  diff --git a/b/file_b1 b/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file
  diff --git a/c/d/e/file_cd3 b/c/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/c/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/c/d/file_cd b/c/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/c/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/c/d/file_cd2 b/c/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/c/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file



  $ josh-filter -s :PATHS:/c master --update refs/josh/filtered
  [3] :/c
  [3] :PATHS
  * add file_cd2
      |-- e
      |   `-- file_cd3
      |-- file_cd
      `-- file_cd2
  $ git diff $EMPTY_TREE HEAD
  diff --git a/d/e/file_cd3 b/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/d/file_cd b/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/d/file_cd2 b/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file



  $ josh-filter -s :PATHS:/a master --update refs/josh/filtered
  [3] :/c
  [3] :PATHS
  |-- file_a2
  $ josh-filter -s :PATHS:exclude[:/c]:prefix=x master --update refs/josh/filtered
  [3] :/c
  [3] :PATHS
      |   |-- file_a2
          `-- file_b1
  $ git diff $EMPTY_TREE HEAD
  diff --git a/x/a/file_a2 b/x/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/x/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/x/a/workspace.josh b/x/a/workspace.josh
  new file mode 100644
  index 0000000..b5fbe37
  --- /dev/null
  +++ b/x/a/workspace.josh
  @@ -0,0 +1 @@
  +a/workspace.josh
  \ No newline at end of file
  diff --git a/x/b/file_b1 b/x/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/x/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file
  $ josh-filter -s :PATHS master --update refs/josh/filtered
  [3] :/c
  [5] :PATHS
  * edit file_cd3
  * add newfile
  * add file_cd2
  |   |-- file_a2
  |   |-- newfile
      `-- file_b1
  2 directories, 4 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/a/newfile b/a/newfile
  new file mode 100644
  index 0000000..17b95ba
  --- /dev/null
  +++ b/a/newfile
  @@ -0,0 +1 @@
  +a/newfile
  \ No newline at end of file
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..b5fbe37
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1 @@
  +a/workspace.josh
  \ No newline at end of file
  diff --git a/b/file_b1 b/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file




  $ josh-filter -s :PATHS:FOLD master --update refs/josh/filtered
  [3] :/c
  [4] :FOLD
  [5] :PATHS
  * add newfile
  * add file_cd2
  |   |-- file_a2
  |   |-- newfile
  |   `-- file_b1
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  5 directories, 7 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/a/newfile b/a/newfile
  new file mode 100644
  index 0000000..17b95ba
  --- /dev/null
  +++ b/a/newfile
  @@ -0,0 +1 @@
  +a/newfile
  \ No newline at end of file
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..b5fbe37
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1 @@
  +a/workspace.josh
  \ No newline at end of file
  diff --git a/b/file_b1 b/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file
  diff --git a/c/d/e/file_cd3 b/c/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/c/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/c/d/file_cd b/c/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/c/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/c/d/file_cd2 b/c/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/c/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file



  $ josh-filter -s :PATHS:/c:FOLD master --update refs/josh/filtered
  [4] :/c
  [5] :PATHS
  [7] :FOLD
  * add file_cd2
      |-- e
      |   `-- file_cd3
      |-- file_cd
      `-- file_cd2
  $ git diff $EMPTY_TREE HEAD
  diff --git a/d/e/file_cd3 b/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/d/file_cd b/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/d/file_cd2 b/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file



  $ josh-filter -s :PATHS:workspace=a:FOLD master --update refs/josh/filtered
  [2] :workspace=a
  [4] :/c
  [5] :PATHS
  [9] :FOLD
  * add newfile
  |-- file_a2
  |-- newfile
  0 directories, 3 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/file_a2 b/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/newfile b/newfile
  new file mode 100644
  index 0000000..17b95ba
  --- /dev/null
  +++ b/newfile
  @@ -0,0 +1 @@
  +a/newfile
  \ No newline at end of file
  diff --git a/workspace.josh b/workspace.josh
  new file mode 100644
  index 0000000..b5fbe37
  --- /dev/null
  +++ b/workspace.josh
  @@ -0,0 +1 @@
  +a/workspace.josh
  \ No newline at end of file