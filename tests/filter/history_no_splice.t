  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

Create initial state: two directories (a/ and b/) plus a workspace that only covers a/

  $ mkdir a b ws
  $ echo contents1 > a/file1
  $ echo contents2 > b/file2
  $ cat > ws/workspace.josh <<EOF
  > :/a
  > EOF
  $ git add .
  $ git commit -m "initial" 1> /dev/null

Add a commit that only touches a/

  $ echo more > a/file3
  $ git add .
  $ git commit -m "add a/file3" 1> /dev/null

Extend the workspace to include b/ as well. By default, per_rev_filter notices
that b/ became visible because the *filter* changed (not the source tree) and
splices in b/'s prior history as a synthetic merge parent.

  $ cat > ws/workspace.josh <<EOF
  > :[
  >   :/a
  >   :/b
  > ]
  > EOF
  $ git add .
  $ git commit -m "extend workspace to b/" 1> /dev/null

Default: the "extend workspace" commit gets a synthetic parent splicing in b/ history.

  $ josh-filter ":workspace=ws"
  c9b197c2fb88bf281d014f9abea6f61d299b4aa9
  $ git log --graph --pretty=%s FILTERED_HEAD
  *   extend workspace to b/
  |\  
  | * initial
  * add a/file3
  * initial

With history="no-splice": no synthetic parent, so b/ history is not spliced in.

  $ josh-filter ":~(history=\"no-splice\")[:workspace=ws]"
  f4d5b37465c0a8d498fb9e28f7b9728aaa21a654
  $ git log --graph --pretty=%s FILTERED_HEAD
  * extend workspace to b/
  * add a/file3
  * initial
