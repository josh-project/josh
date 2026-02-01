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

  $ git checkout -q -b branch-1
  $ echo "one" > one.txt
  $ git add .
  $ git commit -q -m "change-id: change-1"

  $ git checkout -q -b branch-2
  $ echo "two" > two.txt
  $ git add .
  $ git commit -q -m "change-id: change-2"

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
  Found 2 changes


# Verify the commit was created
  $ git log --oneline
  6f2c9c6 Track remote: myremote
  51f2a63 Initial metarepo commit

# Check the tree structure
  $ git ls-tree --format "${GIT_TREE_FMT}" -r HEAD
  100644 blob c937373cb4421598011a1a58ddab20d6227618e0 init.txt
  100644 blob 2e7f82d91cf9f0819028303d1161e5d4449867a4 remotes/myremote/changes.json
  100644 blob 6356f6b63a72d736126c941703fc077d41b662ba remotes/myremote/link/.link.josh

# Verify .link.josh content
  $ git show HEAD:remotes/myremote/link/.link.josh
  :~(
      commit="18e9c0f08e192befb8ff07de548ddf5bd41f8e69"
      remote="../remote.git"
      target="HEAD"
  )[
      :/
  ]

  $ git show HEAD:remotes/myremote/changes.json
  {
    "changes": [
      {
        "id": "change-1",
        "head": "28d9779d81e6ccdd85e9145e5b1dae3a6ffd61f4"
      },
      {
        "id": "change-2",
        "head": "ddd79153049e608636bccd727396380ebf0e16de"
      }
    ],
    "edges": [
      {
        "from": "change-2",
        "to": "change-1",
        "base": "28d9779d81e6ccdd85e9145e5b1dae3a6ffd61f4"
      }
    ]
  } (no-eol)
