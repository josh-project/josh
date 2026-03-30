set -e -x

if [ -z "${CARGO_TARGET_DIR}" ]; then
    TARGET_DIR=$(pwd)/target
else
    TARGET_DIR=${CARGO_TARGET_DIR}
fi

OS=$(uname -s)
if [ "$OS" = "Darwin" ]; then
    DATE_CMD="gdate"
else
    DATE_CMD="date"
fi

PATH="${TARGET_DIR}/debug/:${PATH}"
PATH="$(pwd)/scripts/:${PATH}"
export PATH

export JOSH_COMMIT_TIME=0
export GIT_AUTHOR_NAME=Josh
export GIT_AUTHOR_EMAIL=josh@example.com

GIT_AUTHOR_DATE=$(${DATE_CMD} -R -d "2005-04-07T22:13:13Z")
export GIT_AUTHOR_DATE

export GIT_COMMITTER_NAME=Josh
export GIT_COMMITTER_EMAIL=josh@example.com

GIT_COMMITTER_DATE=$(${DATE_CMD} -R -d "2005-04-07T22:13:13Z")
export GIT_COMMITTER_DATE

export EMPTY_TREE="4b825dc642cb6eb9a060e54bf8d69288fbee4904"
CONFIG_FILE=$(mktemp)
trap 'rm ${CONFIG_FILE}' EXIT

export GIT_CONFIG_GLOBAL=${CONFIG_FILE}
git config --global init.defaultBranch master

cargo fmt
export RUST_BACKTRACE=1
python3 -m prysk "$@"
