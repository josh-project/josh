  $ export TESTTMP=${PWD}
  $ export JOSH_STORE=${TESTTMP}/store


# Create a test repository with "main" as the default branch
  $ mkdir -p remote
  $ cd remote
  $ git init -q -b main

# Create main branch with content
  $ mkdir -p sub1 sub2
  $ echo "file1" > sub1/file1
  $ echo "file2" > sub1/file2
  $ echo "file3" > sub2/file3
  $ git add .
  $ git commit -m "Initial commit"
  [main (root-commit) c8050c5] Initial commit
   3 files changed, 3 insertions(+)
   create mode 100644 sub1/file1
   create mode 100644 sub1/file2
   create mode 100644 sub2/file3

# Create another branch
  $ git checkout -b feature-branch
  Switched to a new branch 'feature-branch'
  $ echo "feature content" > sub1/feature.txt
  $ git add .
  $ git commit -m "Feature branch commit"
  [feature-branch 72f7018] Feature branch commit
   1 file changed, 1 insertion(+)
   create mode 100644 sub1/feature.txt

  $ git checkout main
  Switched to branch 'main'
  $ cd ..

# Create a bare repository for cloning
  $ git clone --bare remote remote.git
  Cloning into bare repository 'remote.git'...
  done.
  $ cd ${TESTTMP}

  $ which git
  /opt/git-install/bin/git

Test josh clone with main branch as default

  $ cat remote.git/HEAD
  ref: refs/heads/main

  $ git ls-remote --symref remote.git HEAD
  ref: refs/heads/main\tHEAD (esc)
  c8050c5d4fc5f431e684ca501a7ff3db5aa47103\tHEAD (esc)

  $ josh clone remote.git filtered-repo --filter :/sub1 2>&1 | sed "s|${TESTTMP}|\${TESTTMP}|g"
  Added remote 'remote' at 'remote'
  Cloned worktree to ${TESTTMP}/filtered-repo

  $ cat filtered-repo/.git/HEAD
  ref: refs/heads/main
  $ cat filtered-repo/.git/refs/remotes/remote/HEAD
  ref: refs/remotes/remote/main

  $ cd filtered-repo
  $ find .git | grep HEAD | sort
  .git/FETCH_HEAD
  .git/HEAD
  .git/logs/HEAD
  .git/logs/refs/remotes/remote/HEAD
  .git/refs/remotes/remote/HEAD
  $ git symbolic-ref refs/remotes/remote/HEAD
  refs/remotes/remote/main

# Check that we have the main branch with filtered content
  $ git branch -a
  * main
    remotes/remote/HEAD -> remote/main
    remotes/remote/feature-branch
    remotes/remote/main

# Check that main branch has filtered content
  $ git checkout main
  Already on 'main'
  Your branch is up to date with 'remote/main'.
  $ ls
  file1
  file2

# Check that feature-branch has filtered content
  $ git checkout feature-branch
  branch 'feature-branch' set up to track 'remote/feature-branch'.
  Switched to a new branch 'feature-branch'
  $ ls
  feature.txt
  file1
  file2

# Check the reference structure
  $ tree .git/refs
  .git/refs
  |-- heads
  |   |-- feature-branch
  |   `-- main
  |-- remotes
  |   `-- remote
  |       |-- HEAD
  |       |-- feature-branch
  |       `-- main
  `-- tags
  
  5 directories, 5 files

  $ cd ..
