  $ export TESTTMP=${PWD}

  $ git init -q
  $ git commit -q --allow-empty -m "empty"

  $ josh-filter -i :[:/a,:/b]
  6777de6ebd97608a3c9fbd8e39ad50cb52b4ae89
  $ git read-tree --reset -u 046b1982b3fa906076d7fce31acf11f19ab4c4c3
  $ find . -type f -not -path './.git/*' -exec echo "-- {}" \; -exec cat {} \;
  -- ./subdir
  a (no-eol)
  $ git read-tree --reset -u 6777de6ebd97608a3c9fbd8e39ad50cb52b4ae89
  $ tree
  .
  `-- compose
      |-- 0
      |   `-- subdir
      `-- 1
          `-- subdir
  
  4 directories, 2 files
  $ git diff ${EMPTY_TREE}..6777de6ebd97608a3c9fbd8e39ad50cb52b4ae89
  diff --git a/compose/0/subdir b/compose/0/subdir
  new file mode 100644
  index 0000000..2e65efe
  --- /dev/null
  +++ b/compose/0/subdir
  @@ -0,0 +1 @@
  +a
  \ No newline at end of file
  diff --git a/compose/1/subdir b/compose/1/subdir
  new file mode 100644
  index 0000000..63d8dbd
  --- /dev/null
  +++ b/compose/1/subdir
  @@ -0,0 +1 @@
  +b
  \ No newline at end of file
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
  $ josh-filter -i :[x=:/a:/b:/d,y=:/a:/c:/d]
  9cac689eff79c3a65966083286840d7ea913e918
  $ git read-tree --reset -u 9cac689eff79c3a65966083286840d7ea913e918
  $ tree
  .
  `-- chain
      |-- 0
      |   `-- subdir
      `-- 1
          `-- compose
              |-- 0
              |   `-- chain
              |       |-- 0
              |       |   `-- subdir
              |       `-- 1
              |           `-- chain
              |               |-- 0
              |               |   `-- subdir
              |               `-- 1
              |                   `-- prefix
              `-- 1
                  `-- chain
                      |-- 0
                      |   `-- subdir
                      `-- 1
                          `-- chain
                              |-- 0
                              |   `-- subdir
                              `-- 1
                                  `-- prefix
  
  19 directories, 7 files
  $ git diff ${EMPTY_TREE}..9cac689eff79c3a65966083286840d7ea913e918
  diff --git a/chain/0/subdir b/chain/0/subdir
  new file mode 100644
  index 0000000..2e65efe
  --- /dev/null
  +++ b/chain/0/subdir
  @@ -0,0 +1 @@
  +a
  \ No newline at end of file
  diff --git a/chain/1/compose/0/chain/0/subdir b/chain/1/compose/0/chain/0/subdir
  new file mode 100644
  index 0000000..63d8dbd
  --- /dev/null
  +++ b/chain/1/compose/0/chain/0/subdir
  @@ -0,0 +1 @@
  +b
  \ No newline at end of file
  diff --git a/chain/1/compose/0/chain/1/chain/0/subdir b/chain/1/compose/0/chain/1/chain/0/subdir
  new file mode 100644
  index 0000000..c59d9b6
  --- /dev/null
  +++ b/chain/1/compose/0/chain/1/chain/0/subdir
  @@ -0,0 +1 @@
  +d
  \ No newline at end of file
  diff --git a/chain/1/compose/0/chain/1/chain/1/prefix b/chain/1/compose/0/chain/1/chain/1/prefix
  new file mode 100644
  index 0000000..c1b0730
  --- /dev/null
  +++ b/chain/1/compose/0/chain/1/chain/1/prefix
  @@ -0,0 +1 @@
  +x
  \ No newline at end of file
  diff --git a/chain/1/compose/1/chain/0/subdir b/chain/1/compose/1/chain/0/subdir
  new file mode 100644
  index 0000000..3410062
  --- /dev/null
  +++ b/chain/1/compose/1/chain/0/subdir
  @@ -0,0 +1 @@
  +c
  \ No newline at end of file
  diff --git a/chain/1/compose/1/chain/1/chain/0/subdir b/chain/1/compose/1/chain/1/chain/0/subdir
  new file mode 100644
  index 0000000..c59d9b6
  --- /dev/null
  +++ b/chain/1/compose/1/chain/1/chain/0/subdir
  @@ -0,0 +1 @@
  +d
  \ No newline at end of file
  diff --git a/chain/1/compose/1/chain/1/chain/1/prefix b/chain/1/compose/1/chain/1/chain/1/prefix
  new file mode 100644
  index 0000000..e25f181
  --- /dev/null
  +++ b/chain/1/compose/1/chain/1/chain/1/prefix
  @@ -0,0 +1 @@
  +y
  \ No newline at end of file
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
      ::a/a
      ::b/b
  ]
  $ josh-filter --reverse -p :[:/a:prefix=a,:/b:prefix=b]:exclude[::a/a,::b/b]
  :exclude[
      ::a/a
      ::b/b
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
  $ git read-tree --reset -u ${FILTER_HASH}
  $ tree
  .
  `-- file
  
  1 directory, 1 file
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/file b/file
  new file mode 100644
  index 0000000..4c33073
  --- /dev/null
  +++ b/file
  @@ -0,0 +1 @@
  +file.txt
  \ No newline at end of file

Test ::dest.txt=src.txt (with =, destination=source)
  $ FILTER_HASH=$(josh-filter -i ::dest.txt=src.txt)
  $ git read-tree --reset -u ${FILTER_HASH}
  $ tree
  .
  `-- file
      |-- 0
      `-- 1
  
  2 directories, 2 files
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
  $ git read-tree --reset -u ${FILTER_HASH}
  $ tree
  .
  `-- pattern
  
  1 directory, 1 file
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/pattern b/pattern
  new file mode 100644
  index 0000000..314f02b
  --- /dev/null
  +++ b/pattern
  @@ -0,0 +1 @@
  +*.txt
  \ No newline at end of file

Test ::dir/ (with trailing slash, directory)
  $ FILTER_HASH=$(josh-filter -i ::dir/)
  $ git read-tree --reset -u ${FILTER_HASH}
  $ tree
  .
  `-- chain
      |-- 0
      |   `-- subdir
      `-- 1
          `-- prefix
  
  4 directories, 2 files
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/chain/0/subdir b/chain/0/subdir
  new file mode 100644
  index 0000000..8724519
  --- /dev/null
  +++ b/chain/0/subdir
  @@ -0,0 +1 @@
  +dir
  \ No newline at end of file
  diff --git a/chain/1/prefix b/chain/1/prefix
  new file mode 100644
  index 0000000..8724519
  --- /dev/null
  +++ b/chain/1/prefix
  @@ -0,0 +1 @@
  +dir
  \ No newline at end of file

Test ::a/b/c/ (nested directory path with trailing slash)
  $ FILTER_HASH=$(josh-filter -i ::a/b/c/)
  $ git read-tree --reset -u ${FILTER_HASH}
  $ tree
  .
  `-- chain
      |-- 0
      |   `-- subdir
      `-- 1
          `-- chain
              |-- 0
              |   `-- subdir
              `-- 1
                  `-- chain
                      |-- 0
                      |   `-- subdir
                      `-- 1
                          `-- chain
                              |-- 0
                              |   `-- prefix
                              `-- 1
                                  `-- chain
                                      |-- 0
                                      |   `-- prefix
                                      `-- 1
                                          `-- prefix
  
  16 directories, 6 files
  $ git diff 4b825dc642cb6eb9a060e54bf8d69288fbee4904..${FILTER_HASH}
  diff --git a/chain/0/subdir b/chain/0/subdir
  new file mode 100644
  index 0000000..2e65efe
  --- /dev/null
  +++ b/chain/0/subdir
  @@ -0,0 +1 @@
  +a
  \ No newline at end of file
  diff --git a/chain/1/chain/0/subdir b/chain/1/chain/0/subdir
  new file mode 100644
  index 0000000..63d8dbd
  --- /dev/null
  +++ b/chain/1/chain/0/subdir
  @@ -0,0 +1 @@
  +b
  \ No newline at end of file
  diff --git a/chain/1/chain/1/chain/0/subdir b/chain/1/chain/1/chain/0/subdir
  new file mode 100644
  index 0000000..3410062
  --- /dev/null
  +++ b/chain/1/chain/1/chain/0/subdir
  @@ -0,0 +1 @@
  +c
  \ No newline at end of file
  diff --git a/chain/1/chain/1/chain/1/chain/0/prefix b/chain/1/chain/1/chain/1/chain/0/prefix
  new file mode 100644
  index 0000000..3410062
  --- /dev/null
  +++ b/chain/1/chain/1/chain/1/chain/0/prefix
  @@ -0,0 +1 @@
  +c
  \ No newline at end of file
  diff --git a/chain/1/chain/1/chain/1/chain/1/chain/0/prefix b/chain/1/chain/1/chain/1/chain/1/chain/0/prefix
  new file mode 100644
  index 0000000..63d8dbd
  --- /dev/null
  +++ b/chain/1/chain/1/chain/1/chain/1/chain/0/prefix
  @@ -0,0 +1 @@
  +b
  \ No newline at end of file
  diff --git a/chain/1/chain/1/chain/1/chain/1/chain/1/prefix b/chain/1/chain/1/chain/1/chain/1/chain/1/prefix
  new file mode 100644
  index 0000000..2e65efe
  --- /dev/null
  +++ b/chain/1/chain/1/chain/1/chain/1/chain/1/prefix
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
