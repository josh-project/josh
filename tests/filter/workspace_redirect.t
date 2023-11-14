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

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > EOF
  $ echo "foobar" > ws/extra_file
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ mkdir ws_new
  $ echo "foobar" > ws_new/extra_file_new
  $ cat > ws_new/workspace.josh <<EOF
  > :workspace=ws
  > EOF
  $ git add ws_new
  $ git commit -m "add ws_new" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/heads/filtered
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [2] :workspace=ws
  $ josh-filter -s :workspace=ws_new master --update refs/heads/filtered_new
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [2] :workspace=ws
  [3] :exclude[::ws_new]

  $ git log --graph --pretty=%s refs/heads/filtered
  * add ws
  * add file2
  * add file1
  $ git log --graph --pretty=%s refs/heads/filtered_new
  * add ws
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
  diff --git a/extra_file b/extra_file
  new file mode 100644
  index 0000000..323fae0
  --- /dev/null
  +++ b/extra_file
  @@ -0,0 +1 @@
  +foobar
  diff --git a/sub2/subsub/file2 b/sub2/subsub/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub2/subsub/file2
  @@ -0,0 +1 @@
  +contents1
  diff --git a/workspace.josh b/workspace.josh
  new file mode 100644
  index 0000000..68bf391
  --- /dev/null
  +++ b/workspace.josh
  @@ -0,0 +1,2 @@
  +::sub2/subsub/
  +a = :/sub1
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
  diff --git a/extra_file b/extra_file
  new file mode 100644
  index 0000000..323fae0
  --- /dev/null
  +++ b/extra_file
  @@ -0,0 +1 @@
  +foobar
  diff --git a/sub2/subsub/file2 b/sub2/subsub/file2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/sub2/subsub/file2
  @@ -0,0 +1 @@
  +contents1
  diff --git a/workspace.josh b/workspace.josh
  new file mode 100644
  index 0000000..68bf391
  --- /dev/null
  +++ b/workspace.josh
  @@ -0,0 +1,2 @@
  +::sub2/subsub/
  +a = :/sub1


  $ cat > ws/workspace.josh <<EOF
  > :workspace=ws_new
  > EOF
  $ git add ws
  $ git commit -m "add ws recursion" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/heads/filtered
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [2] :workspace=ws
  [3] :exclude[::ws]
  [3] :exclude[::ws_new]
