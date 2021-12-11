  $ export TESTTMP=${PWD}

  $ . ${TESTDIR}/setup_repo.sh

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
  [1] :/a
  [1] :/b
  [1] :[
      ::a/
      ::b/
  ]
  [1] :prefix=a
  [1] :prefix=b
  [3] :INVERT
  [3] :PATHS
  [3] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [12] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]

  $ josh-filter -s :/ master --check-permission --missing-permission --users users.yaml --groups groups.yaml -u CSchilling -r test --update refs/josh/filtered
  w: Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]), b: Empty
  [1] :/a
  [1] :/b
  [1] :[
      ::a/
      ::b/
  ]
  [1] :prefix=a
  [1] :prefix=b
  [3] :INVERT
  [3] :PATHS
  [3] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [12] _invert
  [16] _paths
  $ git checkout refs/josh/filtered
  Note: switching to 'refs/josh/filtered'.
  
  You are in 'detached HEAD' state. You can look around, make experimental
  changes and commit them, and you can discard any commits you make in this
  state without impacting any branches by switching back to a branch.
  
  If you want to create a new branch to retain commits you create, you may
  do so (now or later) by using -c with the switch command. Example:
  
    git switch -c <new-branch-name>
  
  Or undo this operation with:
  
    git switch -
  
  Turn off this advice by setting config variable advice.detachedHead to false
  
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
  $ josh-filter -s :[::b/,::a/] master --check-permission --users users.yaml --groups groups.yaml -u CSchilling -r test --update refs/josh/filtered
  w: Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]), b: Empty
  [2] :prefix=b
  [3] :PATHS
  [3] :prefix=a
  [4] :/a
  [4] :/b
  [4] :INVERT
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [13] _invert
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
  
  HEAD is now at 3259647 add dirs
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  |-- groups.yaml
  `-- users.yaml
  
  2 directories, 5 files
  $ cat b/file_b1
  contents1
  $ josh-filter -s :[::b/,::a/] master --check-permission --missing-permission --users users.yaml --groups groups.yaml -u CSchilling -r test --update refs/josh/filtered
  w: Compose([Chain(Subdir("a"), Prefix("a")), Chain(Subdir("b"), Prefix("b"))]), b: Empty
  [2] :prefix=b
  [3] :PATHS
  [3] :prefix=a
  [4] :/a
  [4] :/b
  [4] :INVERT
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [13] _invert
  [16] _paths
  $ git checkout refs/josh/filtered
  HEAD is now at 3259647 add dirs
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  |-- groups.yaml
  `-- users.yaml
  
  2 directories, 5 files
  $ git log
  commit 3259647798774e12c657ea4cb057c61b1233165a
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add dirs
  $ cat b/file_b1
  contents1
# doesn't work
  $ josh-filter -s :/ master --check-permission --users users.yaml --groups groups.yaml -u bob -r test --update refs/josh/filtered
  [2] :prefix=b
  [3] :PATHS
  [3] :prefix=a
  [4] :/a
  [4] :/b
  [4] :INVERT
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [13] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]
# works
  $ josh-filter -s :/ master --check-permission --users users.yaml --groups groups.yaml -u LMG -r test --update refs/josh/filtered
  w: Nop, b: Empty
  [2] :prefix=b
  [3] :PATHS
  [3] :prefix=a
  [4] :/a
  [4] :/b
  [4] :INVERT
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [13] _invert
  [16] _paths

  $ git diff $EMPTY_TREE HEAD
  diff --git a/a/file_a2 b/a/file_a2
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/a/file_a2
  @@ -0,0 +1 @@
  +contents1
  diff --git a/a/workspace.josh b/a/workspace.josh
  new file mode 100644
  index 0000000..3af54d0
  --- /dev/null
  +++ b/a/workspace.josh
  @@ -0,0 +1 @@
  +cws = :/c
  diff --git a/b/file_b1 b/b/file_b1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/b/file_b1
  @@ -0,0 +1 @@
  +contents1


  $ josh-filter -s :PATHS:workspace=a:INVERT master --update refs/josh/filtered
  [2] :prefix=b
  [3] :PATHS
  [3] :prefix=a
  [3] :workspace=a
  [4] :/a
  [4] :/b
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [7] :INVERT
  [16] _paths
  [23] _invert

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
  [2] :prefix=b
  [3] :FOLD
  [3] :PATHS
  [3] :prefix=a
  [3] :workspace=a
  [4] :/a
  [4] :/b
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [7] :INVERT
  [16] _paths
  [23] _invert



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
  [2] :prefix=b
  [3] :FOLD
  [3] :prefix=a
  [3] :workspace=a
  [4] :/a
  [4] :/b
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :PATHS
  [7] :INVERT
  [19] _paths
  [23] _invert

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
  [2] :prefix=b
  [3] :prefix=a
  [3] :workspace=a
  [4] :/a
  [4] :/b
  [4] :/c
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :PATHS
  [6] :FOLD
  [7] :INVERT
  [19] _paths
  [23] _invert

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
  [2] :prefix=b
  [3] :prefix=a
  [4] :/a
  [4] :/b
  [4] :/c
  [4] :[
      ::a/
      ::b/
  ]
  [4] :subtract[
          :/
          :[
              ::a/
              ::b/
          ]
      ]
  [5] :PATHS
  [5] :workspace=a
  [7] :INVERT
  [10] :FOLD
  [19] _paths
  [23] _invert

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

