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
  > @@ -2,5 +2,7 @@ tree 15e3a4de9f0b90057746be6658b0f321f4bcc470
  >  parent 7d7c9293be5483ccd1a24bdf33ad52cf07cda738
  >  author Josh <josh@example.com> 1112911993 +0000
  >  committer Josh <josh@example.com> 1112911993 +0000
  > +custom-header and value
  > +another-header such that it sorts before custom-header
  >  
  >  second
  > EOF
  patching file commit.txt
  $ new_commit=$(git hash-object --literally -w -t commit commit.txt)
  $ echo $new_commit
  fcb8effd63b724bfaaa173ffb7b475bdb4598a1e
  $ git update-ref refs/heads/master $new_commit

  $ josh-filter --update refs/heads/filtered ':replace("hello":"bye")'

  $ git diff ${EMPTY_TREE}..refs/heads/filtered
  diff --git a/hw.txt b/hw.txt
  new file mode 100644
  index 0000000..0907563
  --- /dev/null
  +++ b/hw.txt
  @@ -0,0 +1 @@
  +bye world
  diff --git a/subdir/hw.txt b/subdir/hw.txt
  new file mode 100644
  index 0000000..9762554
  --- /dev/null
  +++ b/subdir/hw.txt
  @@ -0,0 +1 @@
  +bye moon

  $ git cat-file -p refs/heads/filtered
  tree 3e84d1eef0d5eb144c0c17d7ca57a880fe12a5af
  parent 4a277978f4fd37719f92f0814518df2ca115de42
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  custom-header and value
  another-header such that it sorts before custom-header
  
  second

Need to create a ref to be updated that is DIFFERENT from master so we can see the original commit and its headers are
restored.  Merely updating master would not show that the original commit and its headers are restored because it would
look that way if the filter hadn't run in reverse at all
  $ git update-ref refs/heads/reversed 7d7c929

Now reverse original filter writing back to that ref so we can see that the original commit and its headers are restored
  $ josh-filter --update refs/heads/reversed --reverse ':replace("hello":"bye")'

  $ git log --oneline --all --decorate
  0a2ba91 (filtered) second
  fcb8eff (HEAD -> master, reversed) second
  4a27797 initial
