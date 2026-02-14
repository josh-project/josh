  $ export JOSH_TEST_UI=1
  $ . ${TESTDIR}/setup_test_env.sh

  $ cd ${TESTTMP}
  $ curl -s -I http://127.0.0.1:8002/
  HTTP/1.1 303 See Other\r (esc)
  location: /~/ui/\r (esc)
  content-length: 0\r (esc)
  date: *\r (esc) (glob)
  \r (esc)
  $ curl -s -I http://127.0.0.1:8002/~/ui/index.html
  HTTP/1.1 200 OK\r (esc)
  content-type: text/html\r (esc)
  accept-ranges: bytes\r (esc)
  last-modified: *\r (esc) (glob)
  content-length: *\r (esc) (glob)
  date: * (glob)
  \r (esc)
  $ curl -s -I http://127.0.0.1:8002/~/ui/favicon.ico
  HTTP/1.1 200 OK\r (esc)
  content-type: image/x-icon\r (esc)
  accept-ranges: bytes\r (esc)
  last-modified: *\r (esc) (glob)
  content-length: *\r (esc) (glob)
  date: *\r (esc) (glob)
  \r (esc)
  $ curl -s -I http://127.0.0.1:8002/a/repo
  HTTP/1.1 404 Not Found\r (esc)
  content-length: 0\r (esc)
  date: *\r (esc) (glob)
  \r (esc)
