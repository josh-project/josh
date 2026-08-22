#!/bin/bash
curl -s http://localhost:8002/filters/refresh
killall -2 josh-proxy
# Wait for the shutdown to finish before listing the repository: josh-proxy packs the objects it
# still holds in memory on the way out, so listing while it is still exiting is a race.
for _ in $(seq 100); do
  killall -0 josh-proxy >/dev/null 2>&1 || break
  sleep 0.1
done
cd ${TESTTMP}/remote/scratch
tree -I hooks

