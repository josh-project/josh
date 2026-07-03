set -e

cd /josh/
RUST_BACKTRACE=1 josh-proxy --gc --local=/opt/cache/git/ --remote="${JOSH_REMOTE}" ${JOSH_EXTRA_OPTS}
