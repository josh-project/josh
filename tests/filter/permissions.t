  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init 1> /dev/null

  $ mkdir a
  $ echo "cws = :/c" > a/workspace.josh
  $ echo contents1 > a/file_a2
  $ git add a

  $ mkdir b
  $ echo contents1 > b/file_b1
  $ git add b

  $ mkdir -p c/d
  $ echo contents1 > c/d/file_cd
  $ git add c
  $ git commit -m "add dirs" 1> /dev/null

  $ echo contents2 > c/d/file_cd2
  $ git add c
  $ git commit -m "add file_cd2" 1> /dev/null

  $ mkdir -p c/d/e
  $ echo contents2 > c/d/e/file_cd3
  $ git add c
  $ git commit -m "add file_cd3" 1> /dev/null

  $ echo contents3 >> c/d/e/file_cd3
  $ git add c
  $ git commit -m "edit file_cd3" 1> /dev/null

  $ git log --graph --pretty=%s
  * edit file_cd3
  * add file_cd3
  * add file_cd2
  * add dirs

  $ josh-filter -s :PATHS master --update refs/josh/filtered
  perm Empty
  filter Paths
  [3] :PATHS
  [16] _paths

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 6 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..c9acb10
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1,2 @@
  +#a/workspace.josh
  +cws = :/c
  diff --git a/b/file_b1 b/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file
  diff --git a/c/d/e/file_cd3 b/c/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/c/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/c/d/file_cd b/c/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/c/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/c/d/file_cd2 b/c/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/c/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file



  $ josh-filter -s :PATHS:/c master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Subdir("c"))
  [3] :/c
  [3] :PATHS
  [16] _paths

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- d
      |-- e
      |   `-- file_cd3
      |-- file_cd
      `-- file_cd2
  
  2 directories, 3 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/d/e/file_cd3 b/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/d/file_cd b/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/d/file_cd2 b/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file



  $ josh-filter -s :PATHS:/a master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Subdir("a"))
  [1] :/a
  [3] :/c
  [3] :PATHS
  [16] _paths

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- file_a2
  `-- workspace.josh
  
  0 directories, 2 files


  $ josh-filter -s :PATHS:exclude[::c/]:prefix=x master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Chain(Exclude(Chain(Subdir("c"), Prefix("c"))), Prefix("x")))
  [1] :/a
  [1] :exclude[::c/]
  [1] :prefix=x
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [16] _paths

  $ git log --graph --pretty=%s refs/josh/filtered
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  `-- x
      |-- a
      |   |-- file_a2
      |   `-- workspace.josh
      `-- b
          `-- file_b1
  
  3 directories, 3 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/x/a/file_a2 b/x/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/x/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/x/a/workspace.josh b/x/a/workspace.josh
  new file mode 100644
  index 0000000..c9acb10
  --- /dev/null
  +++ b/x/a/workspace.josh
  @@ -0,0 +1,2 @@
  +#a/workspace.josh
  +cws = :/c
  diff --git a/x/b/file_b1 b/x/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/x/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file


  $ josh-filter -s :PATHS:INVERT master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Invert)
  [1] :/a
  [1] :exclude[::c/]
  [1] :prefix=x
  [3] :/c
  [3] :INVERT
  [3] :PATHS
  [3] :prefix=c
  [12] _invert
  [16] _paths

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 6 files
  $ cat a/file_a2
  a/file_a2 (no-eol)
  $ cat b/file_b1
  b/file_b1 (no-eol)


# default permissions give everything
  $ josh-filter -s :/ master --check-permission --update refs/josh/filtered
  perm Empty
  filter Nop
  [1] :/a
  [1] :exclude[::c/]
  [1] :prefix=x
  [3] :/c
  [3] :INVERT
  [3] :PATHS
  [3] :prefix=c
  [12] _invert
  [16] _paths

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 6 files


# default same as this
  $ josh-filter -s :/ master --check-permission -b :empty -w :nop --update refs/josh/filtered_2
  perm Empty
  filter Nop
  [1] :/a
  [1] :exclude[::c/]
  [1] :prefix=x
  [3] :/c
  [3] :INVERT
  [3] :PATHS
  [3] :prefix=c
  [12] _invert
  [16] _paths

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 6 files


# no permissions
  $ josh-filter -s :/ master --check-permission -b :nop -w :empty --update refs/josh/filtered
  perm Chain(Paths, Invert)
  [1] :/a
  [1] :exclude[::c/]
  [1] :prefix=x
  [3] :/c
  [3] :INVERT
  [3] :PATHS
  [3] :prefix=c
  [12] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]
  $ josh-filter -s :/b master --check-permission -w ::a/ --update refs/josh/filtered
  perm Chain(Paths, Chain(Subdir("b"), Chain(Invert, Subtract(Nop, Chain(Subdir("a"), Prefix("a"))))))
  [1] :/b
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [2] :/a
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [4] :INVERT
  [13] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]


  $ josh-filter -s :/b master --check-permission -b ::b/ -w ::b/ --update refs/josh/filtered
  perm Chain(Paths, Chain(Subdir("b"), Chain(Invert, Compose([Chain(Subdir("b"), Prefix("b")), Subtract(Nop, Chain(Subdir("b"), Prefix("b")))]))))
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=b
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :/a
  [2] :/b
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [4] :INVERT
  [13] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]


# access granted
  $ josh-filter -s :/b master --check-permission -w ::b/ --update refs/josh/filtered
  perm Chain(Paths, Chain(Subdir("b"), Chain(Invert, Subtract(Nop, Chain(Subdir("b"), Prefix("b"))))))
  filter Subdir("b")
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=b
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :/a
  [3] :/b
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [4] :INVERT
  [13] _invert
  [16] _paths


# acl
  $ cat << EOF > users.yaml
  > LMG:
  >     groups: ["dev"]
  > CSchilling:
  >     groups: ["dev2"]
  > EOF
  $ cat << EOF > groups.yaml
  > test:
  >     dev:
  >         whitelist: ":/"
  >         blacklist: ":empty"
  >     dev2:
  >         blacklist: ":empty"
  >         whitelist: |
  >             :[
  >                 ::b/
  >                 ::a/
  >             ]
  > EOF

# doesn't work
  $ josh-filter -s :/ master --check-permission --users users.yaml --groups groups.yaml -u CSchilling -r test --update refs/josh/filtered
  w: Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]), b: Empty
  perm Chain(Paths, Chain(Invert, Subtract(Nop, Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]))))
  [1] :[
      ::a/
      ::b/
  ]
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=a
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :prefix=b
  [3] :/a
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [3] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [4] :/b
  [4] :INVERT
  [13] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]

  $ josh-filter -s :/ master --check-permission --missing-permission --users users.yaml --groups groups.yaml -u CSchilling -r test --update refs/josh/filtered
  w: Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]), b: Empty
  perm Empty
  filter Chain(Paths, Chain(Invert, Subtract(Nop, Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]))))
  [1] :[
      ::a/
      ::b/
  ]
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=a
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :prefix=b
  [3] :/a
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [3] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [4] :/b
  [4] :INVERT
  [13] _invert
  [16] _paths
  $ git checkout refs/josh/filtered
  Previous HEAD position was f69915b edit file_cd3
  HEAD is now at c6749dc add file_cd3
  $ tree
  .
  |-- c
  |   `-- d
  |       |-- e
  |       |   `-- file_cd3
  |       |-- file_cd
  |       `-- file_cd2
  |-- groups.yaml
  `-- users.yaml
  
  3 directories, 5 files
  $ cat b/file_b1
  cat: b/file_b1: No such file or directory
  [1]
  $ git log 
  commit c6749dc54b9f93d87e04e89900c0ca1e730c0ca4
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file_cd3
  
  commit 58bed947100bda96f7b2a90df2623e1cdee685e5
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file_cd2
  
  commit 838b5164aff95c891164bfc0ed8611dc008c39ea
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add dirs

# works
  $ josh-filter -s :[:/b,:/a] master --check-permission --users users.yaml --groups groups.yaml -u CSchilling -r test --update refs/josh/filtered
  w: Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]), b: Empty
  perm Chain(Paths, Chain(Compose([Subdir("b"), Subdir("a")]), Chain(Invert, Subtract(Nop, Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))])))))
  filter Compose([Subdir("b"), Subdir("a")])
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [5] :INVERT
  [14] _invert
  [16] _paths
  $ git checkout refs/josh/filtered
  Warning: you are leaving 3 commits behind, not connected to
  any of your branches:
  
    c6749dc add file_cd3
    58bed94 add file_cd2
    838b516 add dirs
  
  If you want to keep them by creating a new branch, this may be a good time
  to do so with:
  
   git branch <new-branch-name> c6749dc
  
  HEAD is now at b2040aa add dirs
  $ tree
  .
  |-- file_a2
  |-- file_b1
  |-- groups.yaml
  |-- users.yaml
  `-- workspace.josh
  
  0 directories, 5 files
  $ cat b/file_b1
  cat: b/file_b1: No such file or directory
  [1]
  $ josh-filter -s :[:/b,:/a] master --check-permission --missing-permission --users users.yaml --groups groups.yaml -u CSchilling -r test --update refs/josh/filtered
  w: Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]), b: Empty
  perm Empty
  filter Chain(Paths, Chain(Compose([Subdir("b"), Subdir("a")]), Chain(Invert, Subtract(Nop, Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))])))))
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [5] :INVERT
  [14] _invert
  [16] _paths
  $ git checkout refs/josh/filtered
  HEAD is now at b2040aa add dirs
  $ tree
  .
  |-- file_a2
  |-- file_b1
  |-- groups.yaml
  |-- users.yaml
  `-- workspace.josh
  
  0 directories, 5 files
  $ git log
  commit b2040aafa2d613696e8e0740cd3debd555550c1a
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add dirs
  $ cat b/file_b1
  cat: b/file_b1: No such file or directory
  [1]
# doesn't work
  $ josh-filter -s :/ master --check-permission --users users.yaml --groups groups.yaml -u bob -r test --update refs/josh/filtered
  perm Chain(Paths, Invert)
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [5] :INVERT
  [14] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]
# works
  $ josh-filter -s :/ master --check-permission --users users.yaml --groups groups.yaml -u LMG -r test --update refs/josh/filtered
  w: Nop, b: Empty
  perm Empty
  filter Nop
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [5] :INVERT
  [14] _invert
  [16] _paths

  $ git diff $EMPTY_TREE HEAD
  diff --git a/file_a2 b/file_a2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file_a2
  @@ -0,0 +1 @@
  +contents1
  diff --git a/file_b1 b/file_b1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/file_b1
  @@ -0,0 +1 @@
  +contents1
  diff --git a/workspace.josh b/workspace.josh
  new file mode 100644
  index 0000000..3af54d0
  --- /dev/null
  +++ b/workspace.josh
  @@ -0,0 +1 @@
  +cws = :/c


  $ josh-filter -s :PATHS:workspace=a:INVERT master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Chain(Workspace("a"), Invert))
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :/c
  [3] :PATHS
  [3] :prefix=c
  [3] :workspace=a
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [8] :INVERT
  [16] _paths
  [24] _invert

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- c
  |   `-- d
  |       |-- e
  |       |   `-- file_cd3
  |       |-- file_cd
  |       `-- file_cd2
  |-- groups.yaml
  `-- users.yaml
  
  4 directories, 7 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..ee73843
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +file_a2
  \ No newline at end of file
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..0ab7ce1
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1 @@
  +workspace.josh
  \ No newline at end of file
  diff --git a/c/d/e/file_cd3 b/c/d/e/file_cd3
  new file mode 100644
  index 0000000..ed74419
  --- /dev/null
  +++ b/c/d/e/file_cd3
  @@ -0,0 +1 @@
  +cws/d/e/file_cd3
  \ No newline at end of file
  diff --git a/c/d/file_cd b/c/d/file_cd
  new file mode 100644
  index 0000000..7afa8f7
  --- /dev/null
  +++ b/c/d/file_cd
  @@ -0,0 +1 @@
  +cws/d/file_cd
  \ No newline at end of file
  diff --git a/c/d/file_cd2 b/c/d/file_cd2
  new file mode 100644
  index 0000000..4fbc84d
  --- /dev/null
  +++ b/c/d/file_cd2
  @@ -0,0 +1 @@
  +cws/d/file_cd2
  \ No newline at end of file

  $ josh-filter -s :PATHS:FOLD master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Fold)
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :/c
  [3] :FOLD
  [3] :PATHS
  [3] :prefix=c
  [3] :workspace=a
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [8] :INVERT
  [16] _paths
  [24] _invert



  $ git checkout master 2> /dev/null
  $ git rm -r c/d
  rm 'c/d/e/file_cd3'
  rm 'c/d/file_cd'
  rm 'c/d/file_cd2'
  $ git commit -m "rm" 1> /dev/null

  $ echo contents2 > a/newfile
  $ git add a
  $ git commit -m "add newfile" 1> /dev/null

  $ josh-filter -s :PATHS master --update refs/josh/filtered
  perm Empty
  filter Paths
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :/c
  [3] :FOLD
  [3] :prefix=c
  [3] :workspace=a
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [5] :PATHS
  [8] :INVERT
  [19] _paths
  [24] _invert

  $ git log --graph --pretty=%s master
  * add newfile
  * rm
  * edit file_cd3
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git log --graph --pretty=%s refs/josh/filtered
  * add newfile
  * rm
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   |-- newfile
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  |-- groups.yaml
  `-- users.yaml
  
  2 directories, 6 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/a/newfile b/a/newfile
  new file mode 100644
  index 0000000..17b95ba
  --- /dev/null
  +++ b/a/newfile
  @@ -0,0 +1 @@
  +a/newfile
  \ No newline at end of file
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..c9acb10
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1,2 @@
  +#a/workspace.josh
  +cws = :/c
  diff --git a/b/file_b1 b/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file


  $ git log --graph --pretty=%s refs/josh/filtered
  * add newfile
  * rm
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   |-- newfile
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  |-- groups.yaml
  `-- users.yaml
  
  2 directories, 6 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/a/newfile b/a/newfile
  new file mode 100644
  index 0000000..17b95ba
  --- /dev/null
  +++ b/a/newfile
  @@ -0,0 +1 @@
  +a/newfile
  \ No newline at end of file
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..c9acb10
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1,2 @@
  +#a/workspace.josh
  +cws = :/c
  diff --git a/b/file_b1 b/b/file_b1
  new file mode 100644
  index 0000000..413b4ca
  --- /dev/null
  +++ b/b/file_b1
  @@ -0,0 +1 @@
  +b/file_b1
  \ No newline at end of file



  $ josh-filter -s :PATHS:/c:FOLD master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Chain(Subdir("c"), Fold))
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :prefix=c
  [3] :workspace=a
  [4] :/c
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [5] :PATHS
  [6] :FOLD
  [8] :INVERT
  [19] _paths
  [24] _invert

  $ git log --graph --pretty=%s refs/josh/filtered
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- d
  |   |-- e
  |   |   `-- file_cd3
  |   |-- file_cd
  |   `-- file_cd2
  |-- groups.yaml
  `-- users.yaml
  
  2 directories, 5 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/d/e/file_cd3 b/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/d/file_cd b/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/d/file_cd2 b/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file



  $ josh-filter -s :PATHS:workspace=a:FOLD master --update refs/josh/filtered
  perm Empty
  filter Chain(Paths, Chain(Workspace("a"), Fold))
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :exclude[::c/]
  [1] :prefix=x
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :[
      :/b
      :/a
  ]
  [2] :[
      ::a/
      ::b/
  ]
  [2] :prefix=a
  [2] :prefix=b
  [3] :prefix=c
  [4] :/c
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :/a
  [5] :/b
  [5] :PATHS
  [5] :workspace=a
  [8] :INVERT
  [10] :FOLD
  [19] _paths
  [24] _invert

  $ git log --graph --pretty=%s refs/josh/filtered
  * add newfile
  * add file_cd3
  * add file_cd2
  * add dirs

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- cws
  |   `-- d
  |       |-- e
  |       |   `-- file_cd3
  |       |-- file_cd
  |       `-- file_cd2
  |-- file_a2
  |-- groups.yaml
  |-- newfile
  |-- users.yaml
  `-- workspace.josh
  
  3 directories, 8 files

  $ git diff $EMPTY_TREE HEAD
  diff --git a/cws/d/e/file_cd3 b/cws/d/e/file_cd3
  new file mode 100644
  index 0000000..8719808
  --- /dev/null
  +++ b/cws/d/e/file_cd3
  @@ -0,0 +1 @@
  +c/d/e/file_cd3
  \ No newline at end of file
  diff --git a/cws/d/file_cd b/cws/d/file_cd
  new file mode 100644
  index 0000000..bb36c67
  --- /dev/null
  +++ b/cws/d/file_cd
  @@ -0,0 +1 @@
  +c/d/file_cd
  \ No newline at end of file
  diff --git a/cws/d/file_cd2 b/cws/d/file_cd2
  new file mode 100644
  index 0000000..26318eb
  --- /dev/null
  +++ b/cws/d/file_cd2
  @@ -0,0 +1 @@
  +c/d/file_cd2
  \ No newline at end of file
  diff --git a/file_a2 b/file_a2
  new file mode 100644
  index 0000000..4b2f88e
  --- /dev/null
  +++ b/file_a2
  @@ -0,0 +1 @@
  +a/file_a2
  \ No newline at end of file
  diff --git a/newfile b/newfile
  new file mode 100644
  index 0000000..17b95ba
  --- /dev/null
  +++ b/newfile
  @@ -0,0 +1 @@
  +a/newfile
  \ No newline at end of file
  diff --git a/workspace.josh b/workspace.josh
  new file mode 100644
  index 0000000..c9acb10
  --- /dev/null
  +++ b/workspace.josh
  @@ -0,0 +1,2 @@
  +#a/workspace.josh
  +cws = :/c

