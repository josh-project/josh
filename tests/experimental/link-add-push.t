
  $ export TESTTMP=${PWD}

# Create a bare repository for linking
  $ git init --bare  docs_repo.git
  Initialized empty Git repository in * (glob)
  $ cd ${TESTTMP}

# Create a test repository
  $ mkdir -p main_repo
  $ cd main_repo
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

  $ echo "cooler utility code" > utils/util1.txt
  $ git add .
  $ git commit -m "update some stuff"
  [master a50a1ec] update some stuff
   1 file changed, 1 insertion(+), 1 deletion(-)
  $ echo "updated documentation" > docs/readme.txt
  $ git add .
  $ git commit -m "update some docs"
  [master 4796b18] update some docs
   1 file changed, 1 insertion(+), 1 deletion(-)

  $ josh link add /docs ../docs_repo.git :/some_prefix
  Using local content at 'docs' (*) (glob)
  Added link 'docs' with URL '../docs_repo.git', filter ':/some_prefix', target 'HEAD', and mode 'snapshot'
  Created branch: refs/heads/josh-link

  $ git rebase refs/heads/josh-link
  Successfully rebased and updated refs/heads/master.

  $ cat docs/.link.josh
  :~(
      commit="*" (glob)
      mode="snapshot"
      remote="../docs_repo.git"
      target="HEAD"
  )[
      :/some_prefix
  ]

  $ josh link push /docs
  To ../docs_repo.git
   * [new branch]      49451f0ceb13b6e4130217b4db23e114b529e15b -> master
  

  $ cd ${TESTTMP}

  $ git clone docs_repo.git docs_repo
  Cloning into 'docs_repo'...
  done.

  $ cd docs_repo

  $ git log --graph --pretty=%s:%H
  * update some docs:49451f0ceb13b6e4130217b4db23e114b529e15b
  * Initial commit:0d24cd27434a19674c17b21e47f176f7151a0260

  $ tree
  .
  `-- some_prefix
      `-- readme.txt
  
  2 directories, 1 file

