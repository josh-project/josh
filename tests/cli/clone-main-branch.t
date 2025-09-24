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
  Fetched from remote: origin
  Pulled from remote: origin
  Cloned repository to: filtered-repo

  $ cat filtered-repo/.git/HEAD
  ref: refs/heads/main
  $ cat filtered-repo/.git/refs/remotes/origin/HEAD
  ref: refs/remotes/origin/main

  $ cd filtered-repo
  $ find .git | grep HEAD
  .git/HEAD
  .git/FETCH_HEAD
  .git/refs/remotes/origin/HEAD
  .git/logs/HEAD
  .git/logs/refs/remotes/origin/HEAD
  $ git symbolic-ref refs/remotes/origin/HEAD
  refs/remotes/origin/main

# Check that we have the main branch with filtered content
  $ git branch -a
    feature-branch
  * main
    remotes/origin/HEAD -> origin/main
    remotes/origin/feature-branch
    remotes/origin/main

# Check that main branch has filtered content
  $ git checkout main
  Already on 'main'
  $ ls
  file1
  file2

# Check that feature-branch has filtered content
  $ git checkout feature-branch
  Switched to branch 'feature-branch'
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
  |   `-- remotes
  |       `-- origin
  |           |-- feature-branch
  |           `-- main
  |-- remotes
  |   `-- origin
  |       |-- HEAD
  |       |-- feature-branch
  |       `-- main
  `-- tags
  
  8 directories, 7 files

  $ cd ..
