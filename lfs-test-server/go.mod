module github.com/git-lfs/lfs-test-server

go 1.23.0

require (
	// bolt has been archived in 2017, https://github.com/etcd-io/bbolt is the successor
	github.com/boltdb/bolt v1.3.1
	github.com/gorilla/context v1.1.2
	github.com/gorilla/mux v1.8.1
)

require golang.org/x/sys v0.33.0 // indirect
