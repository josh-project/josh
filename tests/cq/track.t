  $ export GIT_TREE_FMT='%(objectmode) %(objecttype) %(objectname) %(path)'
  $ export TESTTMP=${PWD}

# Create a remote repository
  $ mkdir -p remote
  $ cd remote
  $ git init -q

# Create some content and branches
  $ echo "hello" > hello.txt
  $ git add .
  $ git commit -q -m "Initial commit"

  $ git checkout -q -b feature
  $ echo "feature" > feature.txt
  $ git add .
  $ git commit -q -m "Add feature"

  $ git checkout -q master
  $ cd ..

# Create a bare repository
  $ git clone -q --bare remote remote.git

# Create the metarepo
  $ git init -q metarepo
  $ cd metarepo

# Create an initial commit so we have a HEAD
# TODO: commit something via josh-cq init instead?
  $ echo "metarepo" > init.txt
  $ git add init.txt
  $ git commit -q -m "Initial metarepo commit"

# Track the remote repository
  $ josh-cq track ../remote.git myremote
  Tracked remote 'myremote' at ../remote.git
  Found 2 refs


# Verify the commit was created
  $ git log --oneline
  1fa3a18 Track remote: myremote
  51f2a63 Initial metarepo commit

# Check the tree structure
  $ git ls-tree --format "${GIT_TREE_FMT}" -r HEAD
  100644 blob c937373cb4421598011a1a58ddab20d6227618e0 init.txt
  100644 blob 0e52160a5335b3e4f4766bbaf115140eb4d563bf remotes/myremote/link/.link.josh
  100644 blob f449de1d25b5cf82d369808c54331fc25b8b550d remotes/myremote/refs.json

# Verify .link.josh content
  $ git show HEAD:remotes/myremote/link/.link.josh
  :~(
      commit="18e9c0f08e192befb8ff07de548ddf5bd41f8e69"
      remote="../remote.git"
      target="HEAD"
  )[
      :/
  ] (no-eol)

  $ git show HEAD:remotes/myremote/refs.json
  {
    "refs/heads/feature": "e3b96406f42dd2ad94b3779a1fd4bde3dc5e8661",
    "refs/heads/master": "18e9c0f08e192befb8ff07de548ddf5bd41f8e69"
  } (no-eol)
