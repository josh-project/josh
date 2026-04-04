set -e

mkdir /out/debug
( cd josh-ssh-dev-server; go build -o /out/debug/josh-ssh-dev-server )
