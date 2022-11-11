  $ export TESTTMP=${PWD}

  $ josh-filter -p :/a
  :/a
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
  $ josh-filter -p :[x=:/a:/b:/d,y=:/a:/c:/d]
  :/a:[
      x = :/b/d
      y = :/c/d
  ]
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
