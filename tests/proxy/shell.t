  $ . ${TESTDIR}/setup_test_env.sh
  $ kill -9 $(cat ${TESTTMP}/proxy_pid)
  $ josh-proxy --help
  Usage: josh-proxy [OPTIONS] --remote <REMOTE> --local <LOCAL>
  
  Options:
        --remote <REMOTE>                  
        --local <LOCAL>                    
        --poll <poll>                      
        --gc                               Run git gc during maintenance
        --require-auth                     
        --no-background                    
    -n <N>                                 DEPRECATED - no effect!
        --port <PORT>                      [default: 8000]
    -c, --cache-duration <CACHE_DURATION>  Duration between forced cache refresh [default: 0]
        --filter-prefix <FILTER_PREFIX>    Filter to be prefixed to all queries of this instance
        --http-retry <HTTP_RETRY>          Number of retries for HTTP server errors [default: 3]
    -h, --help                             Print help

  $ josh-proxy --port=8002 --local=../../tmp --remote=http://localhost:8001 > proxy.out 2>&1 &
  $ sleep 1
  $ kill -9 $!
  $ grep "init mirror repo" proxy.out
  * DEBUG josh_proxy::service: init mirror repo: "*/shell.t/../../tmp/mirror" (glob)
  $ rm -Rf ../../tmp
