
  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1>/dev/null
  $ cd repo

  $ echo "hello world" > hw.txt
  $ mkdir subdir
  $ echo "hello moon" > subdir/hw.txt

  $ git add .
  $ git commit -m initial
  [master (root-commit) 79f224d] initial
   2 files changed, 2 insertions(+)
   create mode 100644 hw.txt
   create mode 100644 subdir/hw.txt

  $ git diff ${EMPTY_TREE}..refs/heads/master
  diff --git a/hw.txt b/hw.txt
  new file mode 100644
  index 0000000..3b18e51
  --- /dev/null
  +++ b/hw.txt
  @@ -0,0 +1 @@
  +hello world
  diff --git a/subdir/hw.txt b/subdir/hw.txt
  new file mode 100644
  index 0000000..1b95c6e
  --- /dev/null
  +++ b/subdir/hw.txt
  @@ -0,0 +1 @@
  +hello moon

  $ josh-filter -p ':replace("hello":"bye","^(?P<l>.*(?m))$":"$l!")'
  :replace(
      "hello":"bye"
      "^(?P<l>.*(?m))$":"$l!"
  )
  $ josh-filter --update refs/heads/filtered ':replace("hello":"bye","(?m)^(?P<l>.+)$":"$l!")'

  $ git diff ${EMPTY_TREE}..refs/heads/filtered
  diff --git a/hw.txt b/hw.txt
  new file mode 100644
  index 0000000..9836695
  --- /dev/null
  +++ b/hw.txt
  @@ -0,0 +1 @@
  +bye world!
  diff --git a/subdir/hw.txt b/subdir/hw.txt
  new file mode 100644
  index 0000000..cb72486
  --- /dev/null
  +++ b/subdir/hw.txt
  @@ -0,0 +1 @@
  +bye moon!

  $ josh-filter --update refs/heads/filtered --reverse ':replace("hello":"bye","(?m)^(?P<l>.+)$":"$l!")'

  $ git diff ${EMPTY_TREE}..refs/heads/master
  diff --git a/hw.txt b/hw.txt
  new file mode 100644
  index 0000000..3b18e51
  --- /dev/null
  +++ b/hw.txt
  @@ -0,0 +1 @@
  +hello world
  diff --git a/subdir/hw.txt b/subdir/hw.txt
  new file mode 100644
  index 0000000..1b95c6e
  --- /dev/null
  +++ b/subdir/hw.txt
  @@ -0,0 +1 @@
  +hello moon

  $ josh-filter --update refs/heads/filtered ':[xdir=:/subdir,:replace("hello":"bye","(?m)^(?P<l>.+)$":"$l!")]'
  $ git diff ${EMPTY_TREE}..refs/heads/filtered
  diff --git a/hw.txt b/hw.txt
  new file mode 100644
  index 0000000..9836695
  --- /dev/null
  +++ b/hw.txt
  @@ -0,0 +1 @@
  +bye world!
  diff --git a/xdir/hw.txt b/xdir/hw.txt
  new file mode 100644
  index 0000000..1b95c6e
  --- /dev/null
  +++ b/xdir/hw.txt
  @@ -0,0 +1 @@
  +hello moon
