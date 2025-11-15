  $ export TESTTMP=${PWD}

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

  $ josh clone remote.git :/sub1 filtered-repo
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  From file://${TESTTMP}/remote
   * [new branch]      feature-branch -> refs/josh/remotes/origin/feature-branch
   * [new branch]      main           -> refs/josh/remotes/origin/main
  
  From file://${TESTTMP}/filtered-repo
   * [new branch]      feature-branch -> origin/feature-branch
   * [new branch]      main           -> origin/main
  
  Fetched from remote: origin
  Switched to a new branch 'main'
  
  Cloned repository to: ${TESTTMP}/filtered-repo

  $ cat filtered-repo/.git/HEAD
  ref: refs/heads/main
  $ cat filtered-repo/.git/refs/remotes/origin/HEAD
  ref: refs/remotes/origin/main

  $ cd filtered-repo
  $ find .git | grep HEAD | sort
  .git/FETCH_HEAD
  .git/HEAD
  .git/logs/HEAD
  .git/refs/namespaces/josh-origin/HEAD
  .git/refs/remotes/origin/HEAD
  $ git symbolic-ref refs/remotes/origin/HEAD
  refs/remotes/origin/main

# Check that we have the main branch with filtered content
  $ git branch -a
  * main
    remotes/origin/HEAD -> origin/main
    remotes/origin/feature-branch
    remotes/origin/main

# Check that main branch has filtered content
  $ git checkout main
  Already on 'main'
  Your branch is up to date with 'origin/main'.
  $ ls
  file1
  file2

# Check that feature-branch has filtered content
  $ git checkout feature-branch
  branch 'feature-branch' set up to track 'origin/feature-branch'.
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
  |-- josh
  |   |-- 24
  |   |   `-- 0
  |   |       |-- 9d5b5e98dceaf62470a7569949757c9643632621
  |   |       `-- d14715b1358e12e9fb4132036e06049fd1ddf88f
  |   `-- remotes
  |       `-- origin
  |           |-- feature-branch
  |           `-- main
  |-- namespaces
  |   `-- josh-origin
  |       |-- HEAD
  |       `-- refs
  |           `-- heads
  |               |-- feature-branch
  |               `-- main
  |-- remotes
  |   `-- origin
  |       |-- HEAD
  |       |-- feature-branch
  |       `-- main
  `-- tags
  
  14 directories, 12 files

  $ cd ..
