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

  $ josh-filter -p :[:/a,:/b]:[:empty,:/]
  :[
      :/a
      :/b
  ]

  $ josh-filter -p :subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/]]
  a = :[
      y = :subtract[
              :subtract[
                      :/y
                      :/x
                  ]
              :/y
          ]
      z = :subtract[
              :subtract[
                      :/z
                      :/x
                  ]
              :/y
          ]
  ]
  $ josh-filter -p :subtract[a=:[::x/,::y/,::z/],a=:[::x/,::y/]]
  a = :[
      y = :subtract[
              :subtract[
                      :/y
                      :/x
                  ]
              :/y
          ]
      z = :subtract[
              :subtract[
                      :/z
                      :/x
                  ]
              :/y
          ]
  ]
  $ josh-filter -p :subtract[a=:[::x/,::y/],a=:[::x/,::y/]]
  :empty
  $ josh-filter -p :subtract[a=:[::x/,::y/],b=:[::x/,::y/]]
  a/y = :subtract[
          :subtract[
                  :/y
                  :/x
              ]
          :/y
      ]
  $ josh-filter -p :subtract[:/a,:[:/b,:/c,:/d,:/e]]
  :subtract[
          :subtract[
                  :subtract[
                          :subtract[
                                  :/a
                                  :/b
                              ]
                          :/c
                      ]
                  :/d
              ]
          :/e
      ]
  $ josh-filter -p :subtract[:[:/a,:/b,:/c],:/c]
  :[
      :subtract[
              :/a
              :/c
          ]
      :subtract[
              :/b
              :/c
          ]
  ]

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
      :subtract[
              :/b
              :/a
          ]
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
  a/j = :subtract[
          :subtract[
                  :subtract[
                          :/a:subtract[
                                  :subtract[
                                          :subtract[
                                                  :subtract[
                                                          :subtract[
                                                                  :subtract[
                                                                          :/j
                                                                          :/b
                                                                      ]
                                                                  :/j
                                                              ]
                                                          :/x/c++666
                                                      ]
                                                  :/x/d
                                              ]
                                          :/x/gg
                                      ]
                                  :/x/u
                              ]
                          :/m/bs/m2/i/tc/i1
                      ]
                  :/m/bs/m2/i/tc/i2
              ]
          :/m/bs/m2/i/tgt
      ]
  x = :[
      c++666 = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/c++666
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      d = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/d
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      g = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/g
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      gg = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/gg
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      u = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/u
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
  ]
  p/au/bs = :[
      i1 = :subtract[
              :subtract[
                      :subtract[
                              :subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/m/bs/m2/i/tc/i1
                                                                              :/a/b
                                                                          ]
                                                                      :/a/j
                                                                  ]
                                                              :/a/x/c++666
                                                          ]
                                                      :/a/x/d
                                                  ]
                                              :/a/x/gg
                                          ]
                                      :/a/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      i2 = :subtract[
              :subtract[
                      :subtract[
                              :subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/m/bs/m2/i/tc/i2
                                                                              :/a/b
                                                                          ]
                                                                      :/a/j
                                                                  ]
                                                              :/a/x/c++666
                                                          ]
                                                      :/a/x/d
                                                  ]
                                              :/a/x/gg
                                          ]
                                      :/a/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      gt = :subtract[
              :subtract[
                      :subtract[
                              :subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/m/bs/m2/i/tgt
                                                                              :/a/b
                                                                          ]
                                                                      :/a/j
                                                                  ]
                                                              :/a/x/c++666
                                                          ]
                                                      :/a/x/d
                                                  ]
                                              :/a/x/gg
                                          ]
                                      :/a/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
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
  a/j = :subtract[
          :subtract[
                  :subtract[
                          :/a:subtract[
                                  :subtract[
                                          :subtract[
                                                  :subtract[
                                                          :subtract[
                                                                  :subtract[
                                                                          :/j
                                                                          :/b
                                                                      ]
                                                                  :/j
                                                              ]
                                                          :/x/c++666
                                                      ]
                                                  :/x/d
                                              ]
                                          :/x/gg
                                      ]
                                  :/x/u
                              ]
                          :/m/bs/m2/i/tc/i1
                      ]
                  :/m/bs/m2/i/tc/i2
              ]
          :/m/bs/m2/i/tgt
      ]
  x = :[
      c++666 = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/c++666
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      d = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/d
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      g = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/g
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      gg = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/gg
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      u = :subtract[
              :subtract[
                      :subtract[
                              :/a:subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/x/u
                                                                              :/b
                                                                          ]
                                                                      :/j
                                                                  ]
                                                              :/x/c++666
                                                          ]
                                                      :/x/d
                                                  ]
                                              :/x/gg
                                          ]
                                      :/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
  ]
  p/au/bs = :[
      i2 = :subtract[
              :subtract[
                      :subtract[
                              :subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/m/bs/m2/i/tc/i2
                                                                              :/a/b
                                                                          ]
                                                                      :/a/j
                                                                  ]
                                                              :/a/x/c++666
                                                          ]
                                                      :/a/x/d
                                                  ]
                                              :/a/x/gg
                                          ]
                                      :/a/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
      gt = :subtract[
              :subtract[
                      :subtract[
                              :subtract[
                                      :subtract[
                                              :subtract[
                                                      :subtract[
                                                              :subtract[
                                                                      :subtract[
                                                                              :/m/bs/m2/i/tgt
                                                                              :/a/b
                                                                          ]
                                                                      :/a/j
                                                                  ]
                                                              :/a/x/c++666
                                                          ]
                                                      :/a/x/d
                                                  ]
                                              :/a/x/gg
                                          ]
                                      :/a/x/u
                                  ]
                              :/m/bs/m2/i/tc/i1
                          ]
                      :/m/bs/m2/i/tc/i2
                  ]
              :/m/bs/m2/i/tgt
          ]
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
