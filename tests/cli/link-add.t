  $ export TESTTMP=${PWD}

# Create a test repository
  $ mkdir -p remote
  $ cd remote
  $ git init -q
  $ git config user.name "Test User"
  $ git config user.email "test@example.com"

# Create some content
  $ mkdir -p libs utils docs
  $ echo "library code" > libs/lib1.txt
  $ echo "utility code" > utils/util1.txt
  $ echo "documentation" > docs/readme.txt
  $ git add .
  $ git commit -m "Initial commit"
  [master (root-commit) *] Initial commit (glob)
   3 files changed, 3 insertions(+)
   create mode 100644 docs/readme.txt
   create mode 100644 libs/lib1.txt
   create mode 100644 utils/util1.txt

  $ cd ..

# Create a bare repository for linking
  $ git clone --bare remote remote.git
  Cloning into bare repository 'remote.git'...
  done.
  $ cd ${TESTTMP}

# Create a new repository to test link add
  $ git init test-repo
  Initialized empty Git repository in * (glob)
  $ cd test-repo
  $ git config user.name "Test User"
  $ git config user.email "test@example.com"

  $ which git
  /opt/git-install/bin/git

# Create an initial commit so we have a HEAD
  $ echo "initial content" > README.md
  $ git add README.md
  $ git commit -m "Initial commit"
  [master (root-commit) 3eb5c75] Initial commit
   1 file changed, 1 insertion(+)
   create mode 100644 README.md

# Test basic link add with default filter and target
  $ josh link add libs ../remote.git
  Added link 'libs' with URL '*', filter ':/', and target 'HEAD' (glob)
  Created branch: refs/josh/link

# Verify the branch was created
  $ git show-ref | grep refs/josh
  * refs/josh/link (glob)

# Verify HEAD was not updated
  $ git log --oneline
  * Initial commit (glob)

# Check the content of the link branch
  $ git checkout refs/josh/link
  Note: switching to 'refs/josh/link'. (glob)
  * (glob)
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at * Add link: libs (glob)
  $ git ls-tree -r HEAD
  100644 blob f2376e2bab6c5194410bd8a55630f83f933d2f34\tREADME.md (esc)
  100644 blob 206d76fad48424fec1fface3ad37d1c24e5eba3a\tlibs/.josh-link.toml (esc)
  100644 blob dfcaa10d372d874e1cab9c3ba8d0b683099c3826\tlibs/docs/readme.txt (esc)
  100644 blob abe06153eb1e2462265336768a6ecd1164f73ae2\tlibs/libs/lib1.txt (esc)
  100644 blob f03a884ed41c1a40b529001c0b429eed24c5e9e5\tlibs/utils/util1.txt (esc)
  $ cat libs/.josh-link.toml
  remote = "../remote.git"
  branch = "HEAD"
  filter = ":/"
  commit = "d27fa3a10cc019e6aa55fc74c1f0893913380e2d"

  $ git checkout master
  Previous HEAD position was * Add link: libs (glob)
  Switched to branch 'master'

# Test link add with custom filter and target
  $ josh link add utils ../remote.git :/utils --target master
  Added link 'utils' with URL '*', filter ':/utils', and target 'master' (glob)
  Created branch: refs/josh/link

# Verify the branch was created
  $ git show-ref | grep refs/josh
  * refs/josh/link (glob)

# Check the content of the utils link branch
  $ git checkout refs/josh/link
  Note: switching to 'refs/josh/link'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at * Add link: utils (glob)
  $ cat utils/.josh-link.toml
  remote = "../remote.git"
  branch = "master"
  filter = ":/utils"
  commit = "d27fa3a10cc019e6aa55fc74c1f0893913380e2d"

  $ git checkout master
  Previous HEAD position was * Add link: utils (glob)
  Switched to branch 'master'

# Test path normalization (path with leading slash)
  $ josh link add /docs ../remote.git :/docs
  Added link 'docs' with URL '*', filter ':/docs', and target 'HEAD' (glob)
  Created branch: refs/josh/link

# Verify path was normalized (no leading slash in branch name)
  $ git show-ref | grep refs/josh
  * refs/josh/link (glob)


# Test error case - empty path
  $ josh link add "" ../remote.git
  Error: Path cannot be empty
  [1]

# Test error case - not in a git repository
  $ cd ..
  $ josh link add test ../remote.git
  Error: Not in a git repository: * (glob)
  [1]

  $ cd test-repo

# Verify that no git remotes were created (josh link add should not create remotes)
  $ git remote -v

# Verify that no git config entries were created (josh link add should not modify .git/config)
  $ git config --list | grep josh-link
  [1]

# Test help output
  $ josh link --help
  Manage josh links (like `josh remote` but for links)
  
  Usage: josh link <COMMAND>
  
  Commands:
    add    Add a link with optional filter and target branch
    fetch  Fetch from existing link files
    help   Print this message or the help of the given subcommand(s)
  
  Options:
    -h, --help  Print help

  $ josh link add --help
  Add a link with optional filter and target branch
  
  Usage: josh link add [OPTIONS] <PATH> <URL> [FILTER]
  
  Arguments:
    <PATH>    Path where the link will be mounted
    <URL>     Remote repository URL
    [FILTER]  Optional filter to apply to the linked repository
  
  Options:
        --target <TARGET>  Target branch to link (defaults to HEAD)
    -h, --help             Print help

# Test josh link fetch command
# First, create a link file directly in the master branch for testing
  $ mkdir -p test-link
  $ echo 'remote = "../remote.git"' > test-link/.josh-link.toml
  $ echo 'branch = "HEAD"' >> test-link/.josh-link.toml
  $ echo 'filter = ":/test"' >> test-link/.josh-link.toml
  $ echo 'commit = "d27fa3a10cc019e6aa55fc74c1f0893913380e2d"' >> test-link/.josh-link.toml
  $ git add test-link/.josh-link.toml
  $ git commit -m "Add test link file for fetch testing"
  [master *] Add test link file for fetch testing (glob)
   1 file changed, 4 insertions(+)
   create mode 100644 test-link/.josh-link.toml

# Test fetch with specific path
  $ josh link fetch test-link
  Found 1 link file(s) to fetch
  Fetching from link at path: test-link
  Updated 1 link file(s)
  Created branch: refs/josh/link

# Verify the branch was updated
  $ git show-ref | grep refs/josh
  * refs/josh/link (glob)

# Check the updated content
  $ git checkout refs/josh/link
  Note: switching to 'refs/josh/link'. (glob)
  * (glob)
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at * Update links: test-link (glob)
  $ git ls-tree -r HEAD
  100644 blob f2376e2bab6c5194410bd8a55630f83f933d2f34	README.md (esc)
  100644 blob bd917a0bed306891ca07801e3d89b9140954434f	test-link/.josh-link.toml (esc)
  $ cat test-link/.josh-link.toml
  remote = "../remote.git"
  branch = "HEAD"
  filter = ":/test"
  commit = "d27fa3a10cc019e6aa55fc74c1f0893913380e2d"

  $ git checkout master
  Previous HEAD position was * Update links: test-link (glob)
  Switched to branch 'master'

# Test fetch with no path (should find all .josh-link.toml files)
  $ josh link fetch
  Found 1 link file(s) to fetch
  Fetching from link at path: test-link
  Updated 1 link file(s)
  Created branch: refs/josh/link

# Test error case - path that doesn't exist
  $ josh link fetch nonexistent
  Error: Failed to find .josh-link.toml at path 'nonexistent': * (glob)
  [1]

# Test error case - no link files found
  $ cd ..
  $ git init empty-repo
  Initialized empty Git repository in * (glob)
  $ cd empty-repo
  $ git config user.name "Test User"
  $ git config user.email "test@example.com"
  $ echo "initial content" > README.md
  $ git add README.md
  $ git commit -m "Initial commit"
  [master (root-commit) 3eb5c75] Initial commit
   1 file changed, 1 insertion(+)
   create mode 100644 README.md

  $ josh link fetch
  Error: No .josh-link.toml files found
  [1]

  $ cd ..
