set -e

sh run-tests.sh tests || TESTS_FAILED=$?
cp -R ./tests /out/tests
exit ${TESTS_FAILED:-0}
