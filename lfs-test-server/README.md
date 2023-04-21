LFS Test Server
======

[rel]: https://github.com/github/lfs-test-server/releases
[lfs]: https://github.com/github/git-lfs
[api]: https://github.com/github/git-lfs/tree/master/docs/api#readme

LFS Test Server is an example server that implements the [Git LFS API][api]. It
is intended to be used for testing the [Git LFS][lfs] client and is not in a
production ready state.

LFS Test Server is written in Go, with pre-compiled binaries available for Mac,
Windows, Linux, and FreeBSD.

See [CONTRIBUTING.md](CONTRIBUTING.md) for info on working on LFS Test Server and
sending patches.

## Installing

Use the Go installer:

```
  $ go install github.com/git-lfs/lfs-test-server
```


## Building

To build from source, use the Go tools:

```
  $ go get github.com/git-lfs/lfs-test-server
```


## Running

Running the binary will start an LFS server on `localhost:8080` by default.
There are few things that can be configured via environment variables:

    LFS_LISTEN      # The address:port the server listens on, default: "tcp://:8080"
    LFS_HOST        # The host used when the server generates URLs, default: "localhost:8080"
    LFS_METADB      # The database file the server uses to store meta information, default: "lfs.db"
    LFS_CONTENTPATH # The path where LFS files are store, default: "lfs-content"
    LFS_ADMINUSER   # An administrator username, default: not set
    LFS_ADMINPASS   # An administrator password, default: not set
    LFS_CERT        # Certificate file for tls
    LFS_KEY         # tls key
    LFS_SCHEME      # set to 'https' to override default http
    LFS_USETUS      # set to 'true' to enable tusd (tus.io) resumable upload server; tusd must be on PATH, installed separately
    LFS_TUSHOST     # The host used to start the tusd upload server, default "localhost:1080"

If the `LFS_ADMINUSER` and `LFS_ADMINPASS` variables are set, a
rudimentary admin interface can be accessed via
`http://$LFS_HOST/mgmt`. Here you can add and remove users, which must
be done before you can use the server with the client.  If either of
these variables are not set (which is the default), the administrative
interface is disabled.

To use the LFS test server with the Git LFS client, configure it in the repository's `.lfsconfig`:


```
  [lfs]
    url = "http://localhost:8080/"

```

HTTPS:

NOTE: If using https with a self signed cert also disable cert checking in the client repo.

```
  [lfs]
    url = "https://localhost:8080/"

  [http]
    sslverify = false

```


An example usage:


Generate a key pair
```
openssl req -x509 -sha256 -nodes -days 2100 -newkey rsa:2048 -keyout mine.key -out mine.crt
```

Make yourself a run script

```
#!/bin/bash

set -eu
set -o pipefail


LFS_LISTEN="tcp://:9999"
LFS_HOST="127.0.0.1:9999"
LFS_CONTENTPATH="content"
LFS_ADMINUSER="<cool admin user name>"
LFS_ADMINPASS="<better admin password>"
LFS_CERT="mine.crt"
LFS_KEY="mine.key"
LFS_SCHEME="https"

export LFS_LISTEN LFS_HOST LFS_CONTENTPATH LFS_ADMINUSER LFS_ADMINPASS LFS_CERT LFS_KEY LFS_SCHEME

./lfs-test-server

```

Build the server

```
go build

```

Run

```
bash run.sh

```

Check the managment page

browser: https://localhost:9999/mgmt


