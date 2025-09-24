#!/bin/bash

. "$(dirname "$0")/testlib.sh"

setup_test_env

# Create a test repository with multiple branches
  $ mkdir -p remote
  $ cd remote
  $ git init -q
  $ git config user.name "Test User"
  $ git config user.email "test@example.com"

# Create main branch with content
  $ mkdir -p sub1 sub2
  $ echo "file1" > sub1/file1
  $ echo "file2" > sub1/file2
  $ echo "file3" > sub2/file3
  $ git add .
  $ git commit -m "Initial commit"
  [master (root-commit) c8050c5] Initial commit
   3 files changed, 3 insertions(+)
   create mode 100644 sub1/file1
   create mode 100644 sub1/file2
   create mode 100644 sub2/file3

# Create a branch that will be empty when filtered (never had sub1 content)
  $ git checkout --orphan truly-empty-branch
  Switched to a new branch 'truly-empty-branch'
  $ git rm -rf .
  rm 'sub1/file1'
  rm 'sub1/file2'
  rm 'sub2/file3'
  $ mkdir -p other-dir
  $ echo "other content" > other-dir/file.txt
  $ git add .
  $ git commit -m "Truly empty branch - never had sub1"
  [truly-empty-branch (root-commit) 0907dcd] Truly empty branch - never had sub1
   1 file changed, 1 insertion(+)
   create mode 100644 other-dir/file.txt

# Add another commit to the truly empty branch
  $ echo "more other content" > other-dir/another-file.txt
  $ git add .
  $ git commit -m "Another truly empty branch commit"
  [truly-empty-branch 89922be] Another truly empty branch commit
   1 file changed, 1 insertion(+)
   create mode 100644 other-dir/another-file.txt

# Create a branch that has mixed history - some commits with content, some without
  $ git checkout master
  Switched to branch 'master'
  $ git checkout -b mixed-branch
  Switched to a new branch 'mixed-branch'
# First commit has content that matches filter
  $ echo "mixed content" > sub1/mixed-file.txt
  $ git add .
  $ git commit -m "Mixed branch - has content"
  [mixed-branch 58b3b63] Mixed branch - has content
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/mixed-file.txt

# Second commit removes the content (becomes empty when filtered)
  $ git rm sub1/mixed-file.txt
  rm 'sub1/mixed-file.txt'
  $ mkdir -p other-dir
  $ echo "other content" > other-dir/file.txt
  $ git add .
  $ git commit -m "Mixed branch - no matching content"
  [mixed-branch 7a854d2] Mixed branch - no matching content
   2 files changed, 1 insertion(+), 1 deletion(-)
   create mode 100644 other-dir/file.txt
   delete mode 100644 sub1/mixed-file.txt

# Third commit adds content again
  $ echo "more mixed content" > sub1/another-mixed-file.txt
  $ git add .
  $ git commit -m "Mixed branch - has content again"
  [mixed-branch 51276d8] Mixed branch - has content again
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/another-mixed-file.txt

# Create another branch with content that will be filtered
  $ git checkout master
  Switched to branch 'master'
  $ git checkout -b content-branch
  Switched to a new branch 'content-branch'
  $ echo "newfile" > sub1/newfile
  $ git add .
  $ git commit -m "Content branch commit"
  [content-branch d589567] Content branch commit
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/newfile

  $ git checkout master
  Switched to branch 'master'
  $ cd ..

# Create a bare repository for cloning
  $ git clone --bare remote remote.git
  Cloning into bare repository 'remote.git'...
  done.
  $ cd ${TESTTMP}
  /bin/sh: cd: line 84: can't cd to /home/dev: No such file or directory
  [2]

  $ which git
  /opt/git-install/bin/git

Test josh clone with filter that results in empty tree for some branches

  $ josh clone remote.git :/sub1 filtered-repo
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  Fetched from remote: origin
  Pulled from remote: origin
  Cloned repository to: filtered-repo

  $ cd filtered-repo

# Check that we have the main branch with filtered content
  $ git branch -a
    content-branch
  * master
    mixed-branch
    remotes/origin/HEAD -> origin/master
    remotes/origin/content-branch
    remotes/origin/master
    remotes/origin/mixed-branch

# Check that master branch has filtered content
  $ git checkout master
  Already on 'master'
  $ ls
  file1
  file2

# Check that content-branch has filtered content
  $ git checkout content-branch
  Switched to branch 'content-branch'
  $ ls
  file1
  file2
  newfile

# Check that mixed-branch has filtered content (should exist because it has some commits with content)
  $ git checkout mixed-branch
  Switched to branch 'mixed-branch'
  $ ls
  another-mixed-file.txt
  file1
  file2

# Check that truly-empty-branch should not have a filtered reference
# (it should not exist as a local branch since ALL commits result in empty tree when filtered)
  $ tree .git/refs
  .git/refs
  |-- heads
  |   |-- content-branch
  |   |-- master
  |   `-- mixed-branch
  |-- josh
  |   `-- remotes
  |       `-- origin
  |           |-- content-branch
  |           |-- master
  |           |-- mixed-branch
  |           `-- truly-empty-branch
  |-- remotes
  |   `-- origin
  |       |-- HEAD
  |       |-- content-branch
  |       |-- master
  |       `-- mixed-branch
  `-- tags
  
  8 directories, 11 files

  $ cd ..