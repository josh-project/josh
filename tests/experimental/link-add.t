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
  [master (root-commit) d27fa3a] Initial commit
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
  No local content at 'libs', fetching from remote...
  From ../remote
   * branch            HEAD       -> FETCH_HEAD
  
  Using fetched commit d27fa3a10cc019e6aa55fc74c1f0893913380e2d
  Added link 'libs' with URL '../remote.git', filter ':/', target 'HEAD', and mode 'snapshot'
  Created branch: refs/heads/josh-link

# Verify the branch was created
  $ git show-ref | grep refs/heads/josh-link
  20159bad8939bce913e384abe37f12fef436bbaa refs/heads/josh-link

# Verify HEAD was not updated
  $ git log --oneline
  3eb5c75 Initial commit

# Check the content of the link branch
  $ git checkout refs/heads/josh-link
  Note: switching to 'refs/heads/josh-link'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at 20159ba Add link: libs

  $ git ls-tree -r HEAD
  100644 blob f2376e2bab6c5194410bd8a55630f83f933d2f34\tREADME.md (esc)
  100644 blob 0acb86f56c10bc4f5f4829b850009bf11a0bab9e\tlibs/.link.josh (esc)
  $ cat libs/.link.josh
  :~(
      commit="d27fa3a10cc019e6aa55fc74c1f0893913380e2d"
      mode="snapshot"
      remote="../remote.git"
      target="HEAD"
  )[
      :/
  ]

  $ git checkout master
  Previous HEAD position was 20159ba Add link: libs
  Switched to branch 'master'

# Test link add with custom filter and target
  $ josh link add utils ../remote.git :/utils --target master
  No local content at 'utils', fetching from remote...
  From ../remote
   * branch            master     -> FETCH_HEAD
  
  Using fetched commit d27fa3a10cc019e6aa55fc74c1f0893913380e2d
  Added link 'utils' with URL '../remote.git', filter ':/utils', target 'master', and mode 'snapshot'
  Created branch: refs/heads/josh-link

# Verify the branch was created
  $ git show-ref | grep refs/heads/josh-link
  18e1f2757e519eadfa53bf03e51fb55e5e579808 refs/heads/josh-link

# Check the content of the utils link branch
  $ git checkout refs/heads/josh-link
  Note: switching to 'refs/heads/josh-link'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at 18e1f27 Add link: utils
  $ cat utils/.link.josh
  :~(
      commit="d27fa3a10cc019e6aa55fc74c1f0893913380e2d"
      mode="snapshot"
      remote="../remote.git"
      target="master"
  )[
      :/utils
  ]

  $ git checkout master
  Previous HEAD position was 18e1f27 Add link: utils
  Switched to branch 'master'

# Test path normalization (path with leading slash)
  $ josh link add /docs ../remote.git :/docs
  No local content at 'docs', fetching from remote...
  From ../remote
   * branch            HEAD       -> FETCH_HEAD
  
  Using fetched commit d27fa3a10cc019e6aa55fc74c1f0893913380e2d
  Added link 'docs' with URL '../remote.git', filter ':/docs', target 'HEAD', and mode 'snapshot'
  Created branch: refs/heads/josh-link

# Verify path was normalized (no leading slash in branch name)
  $ git show-ref | grep refs/heads/josh-link
  f185ec3e91d12287f1f12ac9421d09777fb279a3 refs/heads/josh-link

  $ git show refs/heads/josh-link
  commit f185ec3e91d12287f1f12ac9421d09777fb279a3
  Author: JOSH <josh@josh-project.dev>
  Date:   Thu Jan 1 00:00:00 1970 +0000
  
      Add link: docs
  
  diff --git a/docs/.link.josh b/docs/.link.josh
  new file mode 100644
  index 0000000..d1fd533
  --- /dev/null
  +++ b/docs/.link.josh
  @@ -0,0 +1,8 @@
  +:~(
  +    commit="d27fa3a10cc019e6aa55fc74c1f0893913380e2d"
  +    mode="snapshot"
  +    remote="../remote.git"
  +    target="HEAD"
  +)[
  +    :/docs
  +]





# Test error case - empty path
  $ josh link add "" ../remote.git
  Error: Path cannot be empty
  Path cannot be empty
  [1]

# Test error case - not in a git repository
  $ cd ..
  $ josh link add test ../remote.git
  Error: Not in a git repository
  Not in a git repository
  could not find repository at '.'; class=Repository (6); code=NotFound (-3)
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
    add     Add a link with optional filter and target branch
    fetch   Fetch all SHAs referenced in .link.josh files across history
    update  Fetch the latest commit from each linked remote and update .link.josh files
    push    Push the linked repository to its remote using the :export filter
    help    Print this message or the help of the given subcommand(s)
  
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
        --mode <MODE>      Link mode: embedded, snapshot, or pointer (defaults to snapshot) [default: snapshot]
    -h, --help             Print help

# Test josh link fetch command
# First, create a link file directly in the master branch for testing
  $ mkdir -p test-link
  $ echo ':~(branch="HEAD",commit="d27fa3a10cc019e6aa55fc74c1f0893913380e2d",remote="../remote.git")[:/test]' > test-link/.link.josh
  $ git add test-link/.link.josh
  $ git commit -m "Add test link file for fetch testing"
  [master c3f3beb] Add test link file for fetch testing
   1 file changed, 1 insertion(+)
   create mode 100644 test-link/.link.josh

# Test fetch with specific path
  $ josh link update :/test-link
  Found 1 link file(s) to update
  Fetching HEAD from ../remote.git
  From ../remote
   * branch            HEAD       -> FETCH_HEAD
  
  Updated 1 link file(s)
  Updated branch: refs/heads/josh-link

# Verify the branch was updated
  $ git show-ref | grep refs/heads/josh-link
  2263586b2b74deec84d23baf43d92ce96b866d02 refs/heads/josh-link

# Check the updated content
  $ git checkout refs/heads/josh-link
  Note: switching to 'refs/heads/josh-link'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
  HEAD is now at 2263586 Add test link file for fetch testing
  $ git ls-tree -r HEAD
  100644 blob f2376e2bab6c5194410bd8a55630f83f933d2f34	README.md (esc)
  100644 blob 36a20d072b0e5502dad6203627950771eac14d19\ttest-link/.link.josh (esc)
  $ cat test-link/.link.josh
  :~(
      branch="HEAD"
      commit="d27fa3a10cc019e6aa55fc74c1f0893913380e2d"
      mode="pointer"
      remote="../remote.git"
  )[
      :/test
  ]

  $ git checkout master
  Previous HEAD position was 2263586 Add test link file for fetch testing
  Switched to branch 'master'

# Test update with no path (should find all .link.josh files)
  $ josh link update
  Found 1 link file(s) to update
  Fetching HEAD from ../remote.git
  From ../remote
   * branch            HEAD       -> FETCH_HEAD
  
  Updated 1 link file(s)
  Updated branch: refs/heads/josh-link

# Test error case - path that doesn't exist
  $ josh link update :/nonexistent
  Error: No .link.josh files found
  No .link.josh files found
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
  No .link.josh references found in history

  $ cd ..
