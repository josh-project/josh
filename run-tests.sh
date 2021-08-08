set -e
export PATH=$(pwd)/target/debug/:${PATH}
export PATH=$(pwd)/scripts/:${PATH}

export GIT_AUTHOR_NAME=Josh
export GIT_AUTHOR_EMAIL=josh@example.com
export GIT_AUTHOR_DATE="2005-04-07T22:13:13"
export GIT_COMMITTER_NAME=Josh
export GIT_COMMITTER_EMAIL=josh@example.com
export GIT_COMMITTER_DATE="2005-04-07T22:13:13"
export EMPTY_TREE="4b825dc642cb6eb9a060e54bf8d69288fbee4904"

git config --global init.defaultBranch master

python3 -m cram "$@"
