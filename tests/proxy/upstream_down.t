TODO: add exit call to test server


$ . ${TESTDIR}/setup_test_env.sh
$ cd ${TESTTMP}

$ sh -c 'killall hyper-cgi-test-server' 2> /dev/null

$ git clone -q http://someuser:somepass@localhost:8001/real_repo.git
fatal: unable to access 'http://localhost:8001/real_repo.git/': Failed to connect to localhost port 8001: Connection refused
[128]

$ git clone -q http://someuser:somepass@localhost:8002/real_repo.git full_repo
fatal: Authentication failed for 'http://localhost:8002/real_repo.git/'
[128]

$ ls | sort
hyper-cgi-test-server.out
josh-proxy.out
proxy_pid
remote
server_pid

$ bash ${TESTDIR}/destroy_test_env.sh
remote/scratch/refs
|-- heads
`-- tags

2 directories, 0 files
