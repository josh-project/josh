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

PATH="/josh/debug/:/build-go/debug/:${PATH}"
PATH="${TARGET_DIR}/debug/:${TARGET_DIR}:${PATH}"
PATH="$(pwd)/devtools/target/debug/:$(pwd)/scripts/:${PATH}"
if [ -d /devtools-build/debug ]; then
    PATH="/devtools-build/debug/:${PATH}"
fi
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

export RUST_BACKTRACE=1

# Run each test file with scrut, keeping the cram test syntax. On failure,
# re-run the file with "scrut update" so the .t file is rewritten with the
# actual output (like prysk's -i -y). "scrut update" always exits 0, so the
# "scrut test" run decides the overall result.
TESTS=$(find "$@" -name '*.t' | sort)
if [ -z "${TESTS}" ]; then
    echo "no test files found in: $*" >&2
    exit 1
fi

FAILED=0
for t in ${TESTS}; do
    if ! scrut test --cram-compat "$t"; then
        FAILED=1
        scrut update --cram-compat --assume-yes --replace "$t" || true
    fi
done
exit ${FAILED}
