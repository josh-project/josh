set -e

FAILED=0
for bin in /test-build/test-bins/*; do
    echo "==> Running $(basename "$bin")"
    if ! "$bin"; then
        FAILED=$((FAILED + 1))
    fi
done

if [ "$FAILED" -ne 0 ]; then
    echo "==> $FAILED test binar(ies) failed"
    exit 1
fi
