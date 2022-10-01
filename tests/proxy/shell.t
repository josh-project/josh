  $ . ${TESTDIR}/setup_test_env.sh
  $ kill -9 $(cat ${TESTTMP}/proxy_pid)
  $ ${TARGET_DIR}/debug/josh-proxy --help
  josh-proxy 
  
  USAGE:
      josh-proxy [OPTIONS]
  
  OPTIONS:
      -c, --cache-duration <cache-duration>
              Duration between forced cache refresh
  
          --gc
              Run git gc in maintanance
  
      -h, --help
              Print help information
  
          --local <local>
              
  
      -n <n>
              Number of concurrent upstream git fetch/push operations
  
          --no-background
              
  
          --poll <poll>
              
  
          --port <port>
              
  
          --remote <remote>
              
  
          --require-auth
              
  
          --static-resource-proxy-target <static-resource-proxy-target>
              Proxy static resource requests to a different URL

  $ ${TARGET_DIR}/debug/josh-proxy --port=8002 --local=../../tmp --remote=http://localhost:8001 2>&1 > proxy.out &
  $ sleep 1
  $ kill -9 $!
  $ grep "init mirror repo" proxy.out
  * DEBUG josh_proxy: init mirror repo: "/tmp/cramtests-*/shell.t/../../tmp/mirror" (glob)
  $ rm -Rf ../../tmp
