
  $ export TESTTMP=${PWD}

# Create a bare remote that already has content on master (unrelated history)
  $ git init --bare docs_repo.git
  Initialized empty Git repository in * (glob)
  $ git init -q seed_repo
  $ cd seed_repo
  $ git config user.name "Test User"
  $ git config user.email "test@example.com"
  $ mkdir -p some_prefix
  $ echo "old content" > some_prefix/readme.txt
  $ git add .
  $ git commit -m "Seed commit"
  [master (root-commit) 06358b5] Seed commit
   1 file changed, 1 insertion(+)
   create mode 100644 some_prefix/readme.txt
  $ git push ../docs_repo.git HEAD:master
  To ../docs_repo.git
   * [new branch]      HEAD -> master
  $ cd ${TESTTMP}

# Create main repo with different docs content (no common ancestor with remote)
  $ git init -q main_repo
  $ cd main_repo
  $ git config user.name "Test User"
  $ git config user.email "test@example.com"
  $ mkdir docs
  $ echo "new documentation" > docs/readme.txt
  $ git add .
  $ git commit -m "Add docs"
  [master (root-commit) 25440c2] Add docs
   1 file changed, 1 insertion(+)
   create mode 100644 docs/readme.txt

  $ josh link add /docs ../docs_repo.git :/some_prefix
  Using local content at 'docs' (1d48cfeee0a6ac38edeef6f7c1d1449eaea9317c)
  Added link 'docs' with URL '../docs_repo.git', filter ':/some_prefix', target 'HEAD', and mode 'snapshot'
  Created branch: refs/heads/josh-link
  $ git rebase refs/heads/josh-link
  Successfully rebased and updated refs/heads/master.

# Regular push is rejected: remote has unrelated history
  $ josh link push /docs
  To ../docs_repo.git
   ! [rejected]        1d48cfeee0a6ac38edeef6f7c1d1449eaea9317c -> master (fetch first)
  error: failed to push some refs to '../docs_repo.git'
  hint: * (glob)
  hint: * (glob)
  hint: * (glob)
  hint: * (glob)
  hint: * (glob)
  
  Error: Failed to push to '../docs_repo.git'
  Failed to push to '../docs_repo.git'
  Command exited with code 1: git push ../docs_repo.git 1d48cfeee0a6ac38edeef6f7c1d1449eaea9317c:refs/heads/master
  [1]

# Force push overwrites the remote branch
  $ josh link push --force /docs
  To ../docs_repo.git
   + 06358b5...1d48cfe 1d48cfeee0a6ac38edeef6f7c1d1449eaea9317c -> master (forced update)
  

# Verify the remote now has our content
  $ cd ${TESTTMP}
  $ git clone docs_repo.git verify_repo
  Cloning into 'verify_repo'...
  done.
  $ cd verify_repo
  $ cat some_prefix/readme.txt
  new documentation
