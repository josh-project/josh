  $ export TESTTMP=${PWD}

Test File filter with destination path
  $ cd ${TESTTMP}
  $ git init -q test_dest 1> /dev/null
  $ cd test_dest

  $ mkdir -p src/subdir
  $ echo "source content" > src/subdir/original.txt
  $ echo "other file" > src/subdir/other.txt
  $ echo "root file" > src/root.txt
  $ echo "another" > another.txt
  $ git add .
  $ git commit -m "add files" 1> /dev/null

  $ josh-filter -s ::renamed.txt=src/subdir/original.txt master --update refs/josh/master
  [1] ::renamed.txt=src/subdir/original.txt
  [1] sequence_number

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  `-- renamed.txt
  
  1 directory, 1 file
  $ cat renamed.txt
  source content

Test File filter with destination path in subdirectory
  $ cd ${TESTTMP}
  $ git init -q test_dest_subdir 1> /dev/null
  $ cd test_dest_subdir

  $ mkdir -p src/subdir
  $ echo "source content" > src/subdir/original.txt
  $ echo "other file" > src/subdir/other.txt
  $ echo "root file" > src/root.txt
  $ echo "another" > another.txt
  $ git add .
  $ git commit -m "add files" 1> /dev/null

  $ josh-filter -s ::dest/subdir/renamed.txt=src/subdir/original.txt master --update refs/josh/master
  [1] ::dest/subdir/renamed.txt=src/subdir/original.txt
  [1] sequence_number

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  `-- dest
      `-- subdir
          `-- renamed.txt
  
  3 directories, 1 file
  $ cat dest/subdir/renamed.txt
  source content

Test File filter spec formatting with destination path
  $ josh-filter -p ::dest/renamed.txt=src/file.txt
  ::dest/renamed.txt=src/file.txt

Test File filter backward compatibility (no destination path - keeps same path)
  $ cd ${TESTTMP}
  $ git init -q test_backward 1> /dev/null
  $ cd test_backward

  $ mkdir -p src/subdir
  $ echo "content" > src/subdir/file.txt
  $ echo "other file" > src/subdir/other.txt
  $ echo "root file" > src/root.txt
  $ echo "another" > another.txt
  $ git add .
  $ git commit -m "add files" 1> /dev/null

  $ josh-filter -s ::src/subdir/file.txt master --update refs/josh/master
  [1] ::src/subdir/file.txt
  [1] sequence_number

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  `-- src
      `-- subdir
          `-- file.txt
  
  3 directories, 1 file
  $ cat src/subdir/file.txt
  content

Test File filter with destination path --reverse
  $ cd ${TESTTMP}
  $ git init -q test_reverse 1> /dev/null
  $ cd test_reverse

  $ mkdir -p src/subdir
  $ echo "source content" > src/subdir/original.txt
  $ echo "other file" > src/subdir/other.txt
  $ echo "root file" > src/root.txt
  $ echo "another" > another.txt
  $ git add .
  $ git commit -m "add files" 1> /dev/null

  $ josh-filter -s ::renamed.txt=src/subdir/original.txt master --update refs/josh/master
  [1] ::renamed.txt=src/subdir/original.txt
  [1] sequence_number

  $ git checkout refs/josh/master 2> /dev/null
  $ echo "modified content" > renamed.txt
  $ git add renamed.txt
  $ git commit -m "modify file" 1> /dev/null

  $ josh-filter -s ::renamed.txt=src/subdir/original.txt --reverse master --update refs/josh/master
  [1] ::renamed.txt=src/subdir/original.txt
  [1] sequence_number

  $ git checkout master 2> /dev/null
  $ cat src/subdir/original.txt
  source content
  $ cat src/subdir/other.txt
  other file
  $ tree
  .
  |-- another.txt
  `-- src
      |-- root.txt
      `-- subdir
          |-- original.txt
          `-- other.txt
  
  3 directories, 4 files

Test File filter backward compatibility --reverse
  $ cd ${TESTTMP}
  $ git init -q test_reverse_backward 1> /dev/null
  $ cd test_reverse_backward

  $ mkdir -p src/subdir
  $ echo "content" > src/subdir/file.txt
  $ echo "other file" > src/subdir/other.txt
  $ echo "root file" > src/root.txt
  $ echo "another" > another.txt
  $ git add .
  $ git commit -m "add files" 1> /dev/null

  $ josh-filter -s ::src/subdir/file.txt master --update refs/josh/master
  [1] ::src/subdir/file.txt
  [1] sequence_number

  $ git checkout refs/josh/master 2> /dev/null
  $ echo "modified content" > src/subdir/file.txt
  $ git add src/subdir/file.txt
  $ git commit -m "modify file" 1> /dev/null

  $ josh-filter -s ::src/subdir/file.txt --reverse master --update refs/josh/master
  [1] ::src/subdir/file.txt
  [1] sequence_number

  $ git checkout master 2> /dev/null
  $ cat src/subdir/file.txt
  content
  $ cat src/subdir/other.txt
  other file
  $ tree
  .
  |-- another.txt
  `-- src
      |-- root.txt
      `-- subdir
          |-- file.txt
          `-- other.txt
  
  3 directories, 4 files

