  $ . ${TESTDIR}/setup_test_env.sh
  $ kill -9 $(cat ${TESTTMP}/proxy_pid)
  $ ${TARGET_DIR}/debug/josh-proxy --help
  Usage: josh-proxy [OPTIONS]
  
  Options:
        --remote <remote>
            
        --local <local>
            
        --poll <poll>
            
        --gc
            Run git gc in maintanance
        --require-auth
            
        --no-background
            
    -n <n>
            Number of concurrent upstream git fetch/push operations
        --port <port>
            
    -c, --cache-duration <cache-duration>
            Duration between forced cache refresh
        --static-resource-proxy-target <static-resource-proxy-target>
            Proxy static resource requests to a different URL
    -h, --help
            Print help information

  $ ${TARGET_DIR}/debug/josh-proxy --port=8002 --local=../../tmp --remote=http://localhost:8001 2>&1 > proxy.out &
  $ sleep 1
  $ kill -9 $!
  $ grep "init mirror repo" proxy.out
  * DEBUG josh_proxy: init mirror repo: "/tmp/cramtests-*/shell.t/../../tmp/mirror" (glob)
  $ rm -Rf ../../tmp
