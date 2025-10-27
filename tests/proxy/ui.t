  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}
  $ curl -s -I http://127.0.0.1:8002/
  HTTP/1.1 302 Found\r (esc)
  location: /~/ui/\r (esc)
  date: * (glob)
  \r (esc)
  $ curl -s -I http://127.0.0.1:8002/~/ui/index.html
  HTTP/1.1 200 OK\r (esc)
  etag: * (glob)
  last-modified: * (glob)
  accept-ranges: bytes\r (esc)
  content-length: 633\r (esc)
  content-type: text/html\r (esc)
  date: * (glob)
  \r (esc)
  $ curl -s -I http://127.0.0.1:8002/~/ui/favicon.ico
  HTTP/1.1 200 OK\r (esc)
  etag: * (glob)
  last-modified: * (glob)
  accept-ranges: bytes\r (esc)
  content-length: 12014\r (esc)
  content-type: image/x-icon\r (esc)
  date: * (glob)
  \r (esc)
  $ curl -s -I http://127.0.0.1:8002/a/repo
  HTTP/1.1 302 Found\r (esc)
  location: /~/ui/browse?repo=/a/repo.git&path=&filter=%3A%2F&rev=HEAD\r (esc)
  date: * (glob)
  \r (esc)
