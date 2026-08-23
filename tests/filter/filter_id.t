  $ export TESTTMP=${PWD}

  $ git init -q
  $ git commit -q --allow-empty -m "empty"

  $ FILTER_HASH=$(josh-filter -i :[:/a,:/b])
  $ josh-filter -p ${FILTER_HASH}
  :[
      :/a
      :/b
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── compose/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  a
      └── 1/
          └── subdir/
              └── 0
                  ╵  b
  $ josh-filter -p :/"a"
  :/a
  $ josh-filter --reverse -p :/a
  :prefix=a
  $ josh-filter -p :/a~
  :/a~
  $ josh-filter -p ':/"a%\"$"'
  :/"a%\"$"
  $ josh-filter -p :/a:/b
  :/a/b
  $ josh-filter -p :prefix=x/y:/x
  :prefix=y
  $ josh-filter -p :[:/a:/b,:/a/b]
  :/a/b
  $ josh-filter -p :[:empty,:/a]
  :/a
  $ josh-filter --reverse -p :[:empty,:/a]
  :prefix=a
  $ FILTER_HASH=$(josh-filter -i :[x=:/a:/b:/d,y=:/a:/c:/d])
  $ josh-filter -p ${FILTER_HASH}
  :/a:[
      x = :/b/d
      y = :/c/d
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── chain/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  a
      └── 1/
          └── compose/
              ├── 0/
              │   └── chain/
              │       ├── 0/
              │       │   └── subdir/
              │       │       └── 0
              │       │           ╵  b
              │       ├── 1/
              │       │   └── subdir/
              │       │       └── 0
              │       │           ╵  d
              │       └── 2/
              │           └── prefix/
              │               └── 0
              │                   ╵  x
              └── 1/
                  └── chain/
                      ├── 0/
                      │   └── subdir/
                      │       └── 0
                      │           ╵  c
                      ├── 1/
                      │   └── subdir/
                      │       └── 0
                      │           ╵  d
                      └── 2/
                          └── prefix/
                              └── 0
                                  ╵  y
  $ josh-filter --reverse -p :[x=:/a:/b:/d,y=:/a:/c:/d]
  a = :[
      b/d = :/x
      c/d = :/y
  ]
  $ josh-filter -p :exclude[:/a:/b]
  :exclude[:/a/b]
  $ josh-filter -p :exclude[:/a,:/b]
  :exclude[
      :/a
      :/b
  ]
  $ josh-filter --reverse -p :exclude[:/a,:/b]
  :exclude[
      :prefix=a
      :prefix=b
  ]
  $ josh-filter -p :exclude[::a/,::b/]
  :exclude[
      ::a/
      ::b/
  ]
  $ josh-filter --reverse -p :exclude[::a/,::b/]
  :exclude[
      ::a/
      ::b/
  ]
  $ josh-filter -p :[::a,::b]:/c
  :[
      ::a:/c
      ::b:/c
  ]
  $ josh-filter -p :[::a,::b]::c
  :[
      ::a
      ::b
  ]::c
Exclude of compose should not be split out
  $ josh-filter -p :[:/a:prefix=a,:/b:prefix=b]:exclude[::a/a,::b/b]
  :[
      ::a/
      ::b/
  ]:exclude[
      a = :/a::a
      b = :/b::b
  ]
  $ josh-filter --reverse -p :[:/a:prefix=a,:/b:prefix=b]:exclude[::a/a,::b/b]
  :exclude[
      a = :/a::a
      b = :/b::b
  ]:[
      ::a/
      ::b/
  ]
  $ josh-filter -p :prefix=a/b:prefix=c
  :prefix=c/a/b
  $ josh-filter --reverse -p :prefix=a/b:prefix=c
  :/c/a/b

  $ josh-filter -p :[:/a,:/b]:[:empty,:/]
  :[
      :/a
      :/b
  ]

  $ josh-filter -p :subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/]]
  a/z = :/z
  $ josh-filter -p :subtract[a=:[::x/,::y/,::z/],a=:[::x/,::y/]]
  a/z = :/z
  $ josh-filter -p :subtract[a=:[::x/,::y/],a=:[::x/,::y/]]
  :empty
  $ josh-filter --reverse -p :subtract[a=:[::x/,::y/],a=:[::x/,::y/]]
  :empty
  $ josh-filter -p :subtract[a=:[::x/,::y/],b=:[::x/,::y/]]
  :empty

  $ cat > f <<EOF
  > a/b = :/a/b
  > a/j = :/a/j
  > x/gg = :/a/x/gg
  > x/c++666 = :/a/x/c++666
  > x/g = :/a/x/g
  > p/au/bs/i1 = :/m/bs/m2/i/tc/i1
  > p/au/bs/i2 = :/m/bs/m2/i/tc/i2
  > x/u = :/a/x/u
  > p/au/bs/gt = :/m/bs/m2/i/tgt
  > x/d = :/a/x/d
  > EOF
  $ josh-filter -p --file f
  :/a:[
      a = :[
          ::b/
          ::j/
      ]
      x = :/x:[
          ::c++666/
          ::d/
          ::g/
          ::gg/
          ::u/
      ]
  ]
  p/au/bs = :/m/bs/m2/i:[
      :/tc:[
          ::i1/
          ::i2/
      ]
      gt = :/tgt
  ]

  $ cat > f <<EOF
  > :subtract[:[
  >     ::a/
  >     ::b/
  > ],:[
  >     ::a/
  >     ::c/
  > ]]
  > EOF
  $ josh-filter -p --file f
  b = :subtract[
      :/b
      :/c
  ]

  $ cat > f <<EOF
  > :subtract[
  >     :[
  >         :/a:[
  >             a = :[
  >                 ::b/
  >                 ::j/
  >             ]
  >             x = :/x:[
  >                 ::c++666/
  >                 ::d/
  >                 ::g/
  >                 ::gg/
  >                 ::u/
  >             ]
  >         ]
  >         p/au/bs = :/m/bs/m2/i:[
  >             :/tc:[
  >                 ::i1/
  >                 ::i2/
  >             ]
  >             gt = :/tgt
  >         ]
  >    ],:[
  >         :/a:[
  >             a = :[
  >                 ::b/
  >                 ::j/
  >             ]
  >             x = :/x:[
  >                 ::c++666/
  >                 ::d/
  >                 ::gg/
  >                 ::u/
  >             ]
  >         ]
  >         p/au/bs = :/m/bs/m2/i:[
  >             :/tc:[
  >                 ::i1/
  >                 ::i2/
  >             ]
  >             gt = :/tgt
  >         ]
  >    ]
  > ]
  > EOF

  $ josh-filter -p --file f
  x/g = :/a/x/g

  $ cat > f <<EOF
  > :subtract[
  >     :[
  >         :/a:[
  >             a = :[
  >                 ::b/
  >                 ::j/
  >             ]
  >             x = :/x:[
  >                 ::c++666/
  >                 ::d/
  >                 ::g/
  >                 ::gg/
  >                 ::u/
  >             ]
  >         ]
  >         p/au/bs = :/m/bs/m2/i:[
  >             :/tc:[
  >                 ::i2/
  >             ]
  >             gt = :/tgt
  >         ]
  >    ],:[
  >         :/a:[
  >             a = :[
  >                 ::b/
  >                 ::j/
  >             ]
  >             x = :/x:[
  >                 ::c++666/
  >                 ::d/
  >                 ::gg/
  >                 ::u/
  >             ]
  >         ]
  >         p/au/bs = :/m/bs/m2/i:[
  >             :/tc:[
  >                 ::i1/
  >                 ::i2/
  >             ]
  >             gt = :/tgt
  >         ]
  >    ]
  > ]
  > EOF

  $ josh-filter -p --file f
  x/g = :subtract[
      :/a/x/g
      :/m/bs/m2/i/tc/i1
  ]

  $ cat > f <<EOF
  > a/subsub1 = :/sub1/subsub1
  > a/subsub2 = :/sub1/subsub2
  > EOF

  $ josh-filter -p --file f
  a = :/sub1:[
      ::subsub1/
      ::subsub2/
  ]

Subdir only filters should not reorder filters that share a prefix
  $ cat > f <<EOF
  > a/subsub1 = :/sub1/subsub1
  > :/x/subsub2
  > EOF

  $ josh-filter -p --file f
  a/subsub1 = :/sub1/subsub1
  :/x/subsub2

  $ cat > f <<EOF
  > :/x/subsub2
  > a/subsub1 = :/sub1/subsub1
  > EOF

  $ josh-filter -p --file f
  :/x/subsub2
  a/subsub1 = :/sub1/subsub1

Test File filter tree representations
  $ cd ${TESTTMP}
  $ git init -q test_file_filter_tree 1> /dev/null
  $ cd test_file_filter_tree
  $ git commit -q --allow-empty -m "empty"

Test ::file.txt (single argument, no trailing slash, no =, no *)
  $ FILTER_HASH=$(josh-filter -i ::file.txt)
  $ josh-filter -p ${FILTER_HASH}
  ::file.txt
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── file/
      ├── 0
      │   ╵  file.txt
      └── 1
          ╵  file.txt
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/file/0 b/file/0
  new file mode 100644
  index 0000000..4c33073
  --- /dev/null
  +++ b/file/0
  @@ -0,0 +1 @@
  +file.txt
  \ No newline at end of file
  diff --git a/file/1 b/file/1
  new file mode 100644
  index 0000000..4c33073
  --- /dev/null
  +++ b/file/1
  @@ -0,0 +1 @@
  +file.txt
  \ No newline at end of file

Test ::dest.txt=src.txt (with =, destination=source)
  $ FILTER_HASH=$(josh-filter -i ::dest.txt=src.txt)
  $ josh-filter -p ${FILTER_HASH}
  ::dest.txt=src.txt
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── file/
      ├── 0
      │   ╵  dest.txt
      └── 1
          ╵  src.txt
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/file/0 b/file/0
  new file mode 100644
  index 0000000..e59d527
  --- /dev/null
  +++ b/file/0
  @@ -0,0 +1 @@
  +dest.txt
  \ No newline at end of file
  diff --git a/file/1 b/file/1
  new file mode 100644
  index 0000000..b443386
  --- /dev/null
  +++ b/file/1
  @@ -0,0 +1 @@
  +src.txt
  \ No newline at end of file

Test ::*.txt (with *, pattern)
  $ FILTER_HASH=$(josh-filter -i ::*.txt)
  $ josh-filter -p ${FILTER_HASH}
  ::*.txt
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── pattern/
      └── 0
          ╵  *.txt
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/pattern/0 b/pattern/0
  new file mode 100644
  index 0000000..314f02b
  --- /dev/null
  +++ b/pattern/0
  @@ -0,0 +1 @@
  +*.txt
  \ No newline at end of file

Test ::dir/ (with trailing slash, directory)
  $ FILTER_HASH=$(josh-filter -i ::dir/)
  $ josh-filter -p ${FILTER_HASH}
  ::dir/
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── chain/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  dir
      └── 1/
          └── prefix/
              └── 0
                  ╵  dir
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/chain/0/subdir/0 b/chain/0/subdir/0
  new file mode 100644
  index 0000000..8724519
  --- /dev/null
  +++ b/chain/0/subdir/0
  @@ -0,0 +1 @@
  +dir
  \ No newline at end of file
  diff --git a/chain/1/prefix/0 b/chain/1/prefix/0
  new file mode 100644
  index 0000000..8724519
  --- /dev/null
  +++ b/chain/1/prefix/0
  @@ -0,0 +1 @@
  +dir
  \ No newline at end of file

Test ::a/b/c/ (nested directory path with trailing slash)
  $ FILTER_HASH=$(josh-filter -i ::a/b/c/)
  $ josh-filter -p ${FILTER_HASH}
  ::a/b/c/
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── chain/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  a
      ├── 1/
      │   └── subdir/
      │       └── 0
      │           ╵  b
      ├── 2/
      │   └── subdir/
      │       └── 0
      │           ╵  c
      ├── 3/
      │   └── prefix/
      │       └── 0
      │           ╵  c
      ├── 4/
      │   └── prefix/
      │       └── 0
      │           ╵  b
      └── 5/
          └── prefix/
              └── 0
                  ╵  a
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/chain/0/subdir/0 b/chain/0/subdir/0
  new file mode 100644
  index 0000000..2e65efe
  --- /dev/null
  +++ b/chain/0/subdir/0
  @@ -0,0 +1 @@
  +a
  \ No newline at end of file
  diff --git a/chain/1/subdir/0 b/chain/1/subdir/0
  new file mode 100644
  index 0000000..63d8dbd
  --- /dev/null
  +++ b/chain/1/subdir/0
  @@ -0,0 +1 @@
  +b
  \ No newline at end of file
  diff --git a/chain/2/subdir/0 b/chain/2/subdir/0
  new file mode 100644
  index 0000000..3410062
  --- /dev/null
  +++ b/chain/2/subdir/0
  @@ -0,0 +1 @@
  +c
  \ No newline at end of file
  diff --git a/chain/3/prefix/0 b/chain/3/prefix/0
  new file mode 100644
  index 0000000..3410062
  --- /dev/null
  +++ b/chain/3/prefix/0
  @@ -0,0 +1 @@
  +c
  \ No newline at end of file
  diff --git a/chain/4/prefix/0 b/chain/4/prefix/0
  new file mode 100644
  index 0000000..63d8dbd
  --- /dev/null
  +++ b/chain/4/prefix/0
  @@ -0,0 +1 @@
  +b
  \ No newline at end of file
  diff --git a/chain/5/prefix/0 b/chain/5/prefix/0
  new file mode 100644
  index 0000000..2e65efe
  --- /dev/null
  +++ b/chain/5/prefix/0
  @@ -0,0 +1 @@
  +a
  \ No newline at end of file

Test error cases: mixing * and = (should be errors)
  $ cd ${TESTTMP}
  $ git init -q test_file_filter_errors 1> /dev/null
  $ cd test_file_filter_errors
  $ git commit -q --allow-empty -m "empty"

Test ::*.txt=src.txt (pattern with = should be error)
  $ josh-filter -i ::*.txt=src.txt
  ERROR: Pattern filters cannot use destination=source syntax: *.txt
  [1]

Test ::dest.txt=*.txt (destination=source with pattern in source should be error)
  $ josh-filter -i ::dest.txt=*.txt
  ERROR: Pattern filters not supported in source path: *.txt
  [1]

Test ::*.txt=*.txt (pattern with pattern in source should be error)
  $ josh-filter -i ::*.txt=*.txt
  ERROR: Pattern filters cannot use destination=source syntax: *.txt
  [1]

Test :FOLD
  $ FILTER_HASH=$(josh-filter -i ':FOLD')
  $ josh-filter -p ${FILTER_HASH}
  :FOLD
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── fold

Test :PATHS
  $ FILTER_HASH=$(josh-filter -i ':PATHS')
  $ josh-filter -p ${FILTER_HASH}
  :PATHS
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── paths

Test :INDEX (the filter is experimental; parsing it needs the opt-in)
  $ josh-filter -i ':INDEX' 2>&1 | head -1
  ERROR: :INDEX filter requires JOSH_EXPERIMENTAL_FEATURES=1
  $ FILTER_HASH=$(JOSH_EXPERIMENTAL_FEATURES=1 josh-filter -i ':INDEX')
  $ josh-filter -p ${FILTER_HASH}
  :INDEX
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── index

Test :INVERT
  $ FILTER_HASH=$(josh-filter -i ':INVERT')
  $ josh-filter -p ${FILTER_HASH}
  :INVERT
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── invert

Test :linear
  $ FILTER_HASH=$(josh-filter -i ':linear')
  $ josh-filter -p ${FILTER_HASH}
  :~(
      history="linear"
  )[
      :/
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── meta/
      ├── 0/
      │   └── nop
      └── history
          ╵  linear

  $ FILTER_HASH=$(josh-filter -i ':linear[::x/]')
  $ josh-filter -p ${FILTER_HASH}
  :~(
      history="linear"
  )[
      ::x/
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── meta/
      ├── 0/
      │   └── compose/
      │       └── 0/
      │           └── chain/
      │               ├── 0/
      │               │   └── subdir/
      │               │       └── 0
      │               │           ╵  x
      │               └── 1/
      │                   └── prefix/
      │                       └── 0
      │                           ╵  x
      └── history
          ╵  linear

Test :prune=trivial-merge
  $ FILTER_HASH=$(josh-filter -i ':prune=trivial-merge')
  $ josh-filter -p ${FILTER_HASH}
  :prune=trivial-merge
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── prune
      ╵  trivial-merge

Test :unsign
  $ FILTER_HASH=$(josh-filter -i ':unsign')
  $ josh-filter -p ${FILTER_HASH}
  :~(
      gpgsig="remove"
  )[
      :/
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── meta/
      ├── 0/
      │   └── nop
      └── gpgsig
          ╵  remove

Test :workspace=path/to/workspace
  $ FILTER_HASH=$(josh-filter -i ':workspace=path/to/workspace')
  $ josh-filter -p ${FILTER_HASH}
  :workspace=path/to/workspace
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── workspace/
      └── 0
          ╵  path/to/workspace

Test :+path/to/stored
  $ FILTER_HASH=$(josh-filter -i ':+path/to/stored')
  $ josh-filter -p ${FILTER_HASH}
  :+path/to/stored
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── stored/
      └── 0
          ╵  path/to/stored

Test :hook=hookname
  $ FILTER_HASH=$(josh-filter -i ':hook=hookname')
  $ josh-filter -p ${FILTER_HASH}
  :hook="hookname"
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── hook/
      └── 0
          ╵  hookname

Test :author=Name;email@example.com
  $ FILTER_HASH=$(josh-filter -i ':author="Name";"email@example.com"')
  $ josh-filter -p ${FILTER_HASH}
  :author="Name";"email@example.com"
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── author/
      ├── 0
      │   ╵  Name
      └── 1
          ╵  email@example.com

Test :committer=Name;email@example.com
  $ FILTER_HASH=$(josh-filter -i ':committer="Name";"email@example.com"')
  $ josh-filter -p ${FILTER_HASH}
  :committer="Name";"email@example.com"
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── committer/
      ├── 0
      │   ╵  Name
      └── 1
          ╵  email@example.com

Test :"commit message"
  $ FILTER_HASH=$(josh-filter -i ':"commit message"')
  $ josh-filter -p ${FILTER_HASH}
  :"commit message"
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── message/
      ├── 0
      │   ╵  commit message
      └── 1
          ╵  (?s)^.*$

Test :"commit message";".*"
  $ FILTER_HASH=$(josh-filter -i ':"commit message";".*"')
  $ josh-filter -p ${FILTER_HASH}
  :"commit message";".*"
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── message/
      ├── 0
      │   ╵  commit message
      └── 1
          ╵  .*

Test :pin[:/a]
  $ FILTER_HASH=$(josh-filter -i ':pin[:/a]')
  $ josh-filter -p ${FILTER_HASH}
  :pin[:/a]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── pin/
      └── 0/
          └── subdir/
              └── 0
                  ╵  a

Test :SQUASH
  $ FILTER_HASH=$(josh-filter -i ':SQUASH')
  $ josh-filter -p ${FILTER_HASH}
  :SQUASH
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── squash

Test :replace("pattern":"replacement")
  $ FILTER_HASH=$(josh-filter -i ':replace("pattern":"replacement")')
  $ josh-filter -p ${FILTER_HASH}
  :replace(
      "pattern":"replacement"
  )
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── regex_replace/
      └── 0/
          ├── p
          │   ╵  pattern
          └── r
              ╵  replacement

Test :rev(_:/a)
  $ FILTER_HASH=$(josh-filter -i ':rev(_:/a)')
  $ josh-filter -p ${FILTER_HASH}
  :rev(_:/a)
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── rev/
      └── 0/
          ├── f/
          │   └── subdir/
          │       └── 0
          │           ╵  a
          └── o
              ╵  _


  $ FILTER_HASH=$(josh-filter -i ':~(key1="value1",key2="value2",a="b")[:/sub1]')
  $ josh-filter -p ${FILTER_HASH}
  :~(
      a="b"
      key1="value1"
      key2="value2"
  )[
      :/sub1
  ]
  $ git-tree-pretty ${FILTER_HASH}
  .
  └── meta/
      ├── 0/
      │   └── subdir/
      │       └── 0
      │           ╵  sub1
      ├── a
      │   ╵  b
      ├── key1
      │   ╵  value1
      └── key2
          ╵  value2
