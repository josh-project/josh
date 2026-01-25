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
# TODO: commit something via josh-mq init instead?
  $ echo "metarepo" > init.txt
  $ git add init.txt
  $ git commit -q -m "Initial metarepo commit"

# Track the remote repository
  $ josh-mq track ../remote.git myremote
  From ../remote
   * branch            HEAD       -> FETCH_HEAD
  
  Tracked remote 'myremote' at ../remote.git
  Found 3 refs


# Verify the commit was created
  $ git log --oneline
  c121e35 Track remote: myremote
  51f2a63 Initial metarepo commit

# Check the tree structure
  $ git ls-tree --format "${GIT_TREE_FMT}" -r HEAD
  100644 blob c937373cb4421598011a1a58ddab20d6227618e0 init.txt
  100644 blob 6356f6b63a72d736126c941703fc077d41b662ba remotes/myremote/link/.link.josh
  100644 blob 9225fc196ba1c36efea3fe89d89f3264f20e25c1 remotes/myremote/refs.json

# Verify .link.josh content
  $ git show HEAD:remotes/myremote/link/.link.josh
  :~(
      commit="18e9c0f08e192befb8ff07de548ddf5bd41f8e69"
      remote="../remote.git"
      target="HEAD"
  )[
      :/
  ]

  $ git show HEAD:remotes/myremote/refs.json
  {
    "HEAD": "18e9c0f08e192befb8ff07de548ddf5bd41f8e69",
    "refs/heads/feature": "e3b96406f42dd2ad94b3779a1fd4bde3dc5e8661",
    "refs/heads/master": "18e9c0f08e192befb8ff07de548ddf5bd41f8e69"
  } (no-eol)
