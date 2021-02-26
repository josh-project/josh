set -e
cargo build --all
export PATH=$(pwd)/target/debug/:${PATH}
export PATH=$(pwd)/scripts/:${PATH}

export GIT_AUTHOR_NAME=Josh
export GIT_AUTHOR_EMAIL=josh@example.com
export GIT_AUTHOR_DATE="2005-04-07T22:13:13"
export GIT_COMMITTER_NAME=Josh
export GIT_COMMITTER_EMAIL=josh@example.com
export GIT_COMMITTER_DATE="2005-04-07T22:13:13"

python3 -m cram "$@"
