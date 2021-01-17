  $ export TESTTMP=${PWD}

  $ josh-filter -p :/a
  :/a
  $ josh-filter -p :/a:/b
  :/a/b
  $ josh-filter -p :[:/a:/b,:/a/b]
  :/a/b
  $ josh-filter -p :[x=:/a:/b:/d,y=:/a:/c:/d]
  :/a:[
      x = :/b/d
      y = :/c/d
  ]
  $ josh-filter -p :exclude[:/a:/b]
  :exclude[:/a/b]
  $ josh-filter -p :exclude[:/a,:/b]
  :exclude[
      :/a
      :/b
  ]
  $ josh-filter -p :prefix=a/b:prefix=c
  :prefix=c/a/b

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
  :subtract[
      ::b/
      ::c/
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
  :subtract[
      x/g = :/a/x/g
      p/au/bs/i1 = :/m/bs/m2/i/tc/i1
  ]
