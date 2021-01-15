  $ export TESTTMP=${PWD}

  $ josh-filter -p :/a
  :/a
  $ josh-filter -p :/a:/b
  :/a/b
  $ josh-filter -p :[:/a:/b]
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
