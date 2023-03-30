LFS_LISTEN="tcp://:9999"
LFS_HOST="127.0.0.1:9999"
LFS_CONTENTPATH="/Users/christianschilling/lfs-server-content"
LFS_SCHEME="http"
LFS_PUBLIC="TRUE"

export LFS_LISTEN LFS_HOST LFS_CONTENTPATH LFS_SCHEME LFS_PUBLIC

/Users/christianschilling/go/bin/lfs-test-server
