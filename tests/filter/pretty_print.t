  $ export TESTTMP=${PWD}

  $ josh-filter -p :/a
  :/a
  $ josh-filter -p :/a:/b
  :/a:/b
  $ josh-filter -p :[:/a:/b]
  :/a:/b
  $ josh-filter -p :[:/a:/b:/d,:/a:/c:/d]
  :/a:[
      :/b
      :/c
  ]:/d
  $ josh-filter -p :exclude[:/a:/b]
  :exclude[:/a:/b]
  $ josh-filter -p :exclude[:/a,:/b]
  :exclude[
      :/a
      :/b
  ]
