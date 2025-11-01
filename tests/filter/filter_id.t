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
  $ find . -type f -not -path './.git/*' -exec echo "-- {}" \; -exec cat {} \;
  -- ./compose/0/subdir
  a-- ./compose/1/subdir
  b (no-eol)
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
  $ find . -type f -not -path './.git/*' -exec echo "-- {}" \; -exec cat {} \;
  -- ./chain/0/subdir
  a-- ./chain/1/compose/0/chain/0/subdir
  b-- ./chain/1/compose/0/chain/1/chain/0/subdir
  d-- ./chain/1/compose/0/chain/1/chain/1/prefix
  x-- ./chain/1/compose/1/chain/0/subdir
  c-- ./chain/1/compose/1/chain/1/chain/0/subdir
  d-- ./chain/1/compose/1/chain/1/chain/1/prefix
  y (no-eol)
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
