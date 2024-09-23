  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1>/dev/null
  $ cd repo

  $ echo "hello world" > hw.txt
  $ git add .
  $ git commit -m initial
  [master (root-commit) 7d7c929] initial
   1 file changed, 1 insertion(+)
   create mode 100644 hw.txt

  $ mkdir subdir
  $ echo "hello moon" > subdir/hw.txt
  $ git add .
  $ git commit -m second
  [master bab39f4] second
   1 file changed, 1 insertion(+)
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

Write a custom header into the commit (h/t https://github.com/Byron/gitoxide/blob/68cbea8gix/tests/fixtures/make_pre_epoch_repo.sh#L12-L27)
  $ git cat-file -p @ | tee commit.txt
  tree 15e3a4de9f0b90057746be6658b0f321f4bcc470
  parent 7d7c9293be5483ccd1a24bdf33ad52cf07cda738
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  
  second

  $ patch -p1 <<EOF
  > diff --git a/commit.txt b/commit.txt
  > index 1758866..fe1998a 100644
  > --- a/commit.txt
  > +++ b/commit.txt
  > @@ -2,5 +2,9 @@ tree 15e3a4de9f0b90057746be6658b0f321f4bcc470
  >  parent 7d7c9293be5483ccd1a24bdf33ad52cf07cda738
  >  author Josh <josh@example.com> 1112911993 +0000
  >  committer Josh <josh@example.com> 1112911993 +0000
  > +custom-header with
  > + multiline
  > + value
  > +another-header such that it sorts before custom-header
  >  
  >  second
  > EOF
  patching file commit.txt
  $ new_commit=$(git hash-object --literally -w -t commit commit.txt)
  $ echo $new_commit
  f2fd7b23a4a2318d534d122615a6e75196c3e3c4
  $ git update-ref refs/heads/master $new_commit

  $ josh-filter --update refs/heads/filtered ':prefix=pre'

  $ git diff ${EMPTY_TREE}..refs/heads/filtered
  diff --git a/pre/hw.txt b/pre/hw.txt
  new file mode 100644
  index 0000000..3b18e51
  --- /dev/null
  +++ b/pre/hw.txt
  @@ -0,0 +1 @@
  +hello world
  diff --git a/pre/subdir/hw.txt b/pre/subdir/hw.txt
  new file mode 100644
  index 0000000..1b95c6e
  --- /dev/null
  +++ b/pre/subdir/hw.txt
  @@ -0,0 +1 @@
  +hello moon

  $ git cat-file -p refs/heads/filtered
  tree 6876aad1a2259b9d4c7c24e0e3ff908d3d580404
  parent 73007fa33b8628d6560b78e37191c07c9e001d3b
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  custom-header with
   multiline
   value
  another-header such that it sorts before custom-header
  
  second

  $ josh-filter --update refs/heads/re-filtered ':/pre' refs/heads/filtered 

  $ git show refs/heads/re-filtered
  commit f2fd7b23a4a2318d534d122615a6e75196c3e3c4
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      second
  
  diff --git a/subdir/hw.txt b/subdir/hw.txt
  new file mode 100644
  index 0000000..1b95c6e
  --- /dev/null
  +++ b/subdir/hw.txt
  @@ -0,0 +1 @@
  +hello moon

  $ git cat-file -p refs/heads/re-filtered
  tree 15e3a4de9f0b90057746be6658b0f321f4bcc470
  parent 7d7c9293be5483ccd1a24bdf33ad52cf07cda738
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  custom-header with
   multiline
   value
  another-header such that it sorts before custom-header
  
  second

  $ git log --oneline --all --decorate
  63982dc (filtered) second
  f2fd7b2 (HEAD -> master, re-filtered) second
  73007fa initial
  7d7c929 initial
